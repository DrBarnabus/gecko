# Phase 2 — Outputs, Surfaces, and the Offscreen Target as First-Class

Implementation plan for Phase 2 of the renderer, as sequenced in `ARCHITECTURE.md`. This
document is self-contained: each task can be picked up knowing only `ARCHITECTURE.md` and
the current repository state described here.

Phase 2 introduces the target model and makes the editor's game image a real, owned,
ring-buffered render target rather than an ad-hoc object living in the UI layer. It builds
half the threading seam and the foundation for multiple windows.

> **Deferred from this phase:** the `PresentationSink` trait. In Phase 2 the surface and the
> offscreen target are not symmetric — in the editor the UI writes the swapchain directly while
> the offscreen target is *consumed* by the UI — so a "consume a finished texture" abstraction
> has only one trivial consumer and no shared shape. The sink trait lands in Phase 3 with the
> composite step, where the standalone final-composite blit (and later a capture sink) give it
> real, symmetric consumers. See the note in Step 4 and _Out of scope_.

---

## How to read this document

Each step lists:

- **Rationale** — why the step exists and where it sits in the architecture.
- **Current state** — what exists today, with `file:line` references.
- **Deliverables** — the concrete sub-tasks.
- **Design detail** — proposed types and signatures. Signatures marked _(finalise at
  implementation)_ give the intended shape; minor adjustments during implementation are
  expected and fine.
- **Interactions** — dependencies on other steps.
- **Acceptance** — what must be true to call the step done.

Steps are ordered so that each one compiles, the editor keeps rendering throughout, and the
largest structural moves ride on top of the small stable types introduced first.

---

## Invariants to preserve

These hold across every step and come from Phase 0 and the current code. Do not violate them.

- **RHI ownership boundary.** `gecko_rhi` owns the device, queue, adapter, instance, and all
  live GPU resources, and is the only code that issues GPU operations outside a pass. Upper
  crates hold handles and may name wgpu plain data types (formats, enums) but cannot obtain a
  device. Two sanctioned exceptions: the imgui clones exposed for renderer init
  (`Rhi::device/queue/instance/adapter` at `crates/gecko_rhi/src/lib.rs:157-179`), and
  `SceneRenderer::new`'s device use for shader/pipeline/bind-group construction
  (`crates/gecko_renderer/src/scene_renderer.rs:55`), which stands until the Phase 5
  pipeline cache. Phase 2 must not widen either.
- **Resources are referenced by handles.** New GPU textures go through the registry
  (`ResourceRegistry`, `crates/gecko_rhi/src/resource.rs`) and are referred to by
  `TextureHandle` / `BufferHandle`, never held as raw `wgpu::Texture` above the RHI.
- **Reverse-Z depth.** Depth is `Depth32Float`, compare `Greater`, clear `0.0`. Use the
  constants in `crates/gecko_rhi/src/conventions.rs` (`DEPTH_FORMAT`, `DEPTH_COMPARE`,
  `DEPTH_CLEAR`); never hardcode these values.
- **Bind-group ordering.** Lowest index = lowest change-frequency. Group 0 per-frame, group 1
  per-view, group 2 per-material, group 3 per-draw. The scene pipeline layout already reserves
  this shape (`crates/gecko_renderer/src/scene_renderer.rs:82-86`).
- **The write path is consumer-agnostic.** A pass renders into a `ResolvedTarget` and never
  names what consumes it. Swapping the consumer (UI sampling, a surface blit, a capture) must
  not touch the ring or the scene pass.
- **British English in prose**, ecosystem spelling in identifiers (`color`, `format`).

---

## Target dependency graph

Edges after Phase 2 (arrow = "depends on"):

```
gecko_app (lib + bins) ── App trait, run loop, EditorApp (impl App)
  ├─> gecko_editor  ── imgui bridge + panels (one TextureId per ring slot)
  ├─> gecko_renderer ─ SceneRenderer
  ├─> gecko_rhi ────── RenderTarget, RenderTargetRing, Surface
  ├─> gecko_runtime
  └─> gecko_core

gecko_editor  ─> gecko_renderer, gecko_rhi, gecko_runtime, gecko_core
gecko_renderer ─> gecko_rhi, gecko_core
gecko_rhi ─────> wgpu, slotmap, encase
```

Three decisions ratified for this phase, deviating from a literal reading of `ARCHITECTURE.md`:

1. **No separate run-loop crate.** The run-loop layer is realised as `gecko_app` reshaped into
   a library plus thin binaries, with an `App` seam keeping the loop editor-agnostic. A
   dedicated `gecko_run_loop` crate is deferred until a second binary (standalone game) makes
   the editor-agnostic boundary worth compiler enforcement.
2. **The offscreen target ring lives in `gecko_rhi`, alongside `RenderTarget` and `Surface`.**
   `ARCHITECTURE.md` says "owned in the renderer layer," but that placement was a consequence of
   the game-specific framing. Generalised to a per-frame ring of offscreen targets, it has zero
   renderer dependencies (only `RenderTarget` + `frames_in_flight`), it is pure GPU-resource
   management (rhi's remit), and it mirrors the existing per-slot resource set in
   `FramesInFlight` (`crates/gecko_rhi/src/frame.rs:37-98`). The renderer keeps no target type;
   `SceneRenderer` just renders into a resolved slot. Kept pure: no render-path/view metadata on
   the ring — that is the `View`'s job in Phase 4.
3. **`PresentationSink` is deferred to Phase 3.** Not built in Phase 2 (see the note at the top
   and in Step 4).

---

## Step ordering and why

1. **RenderTarget data + resolve** — smallest concrete type; forces the offscreen textures
   through the registry and gives the scene pass something to consume.
2. **Offscreen target ring into the RHI (single-slot)** — the immediate pain point; retires the
   UI-owned `Viewport`. Kept single-slot so the ownership flip is reviewable alone.
3. **Multi-slot the ring** — sized to `frames_in_flight`; completes the cross-thread handoff
   object and the per-slot UI identity.
4. **Surface into RHI** — relocates the surface, drops the device clone, adds secondary-surface
   creation, and routes present through the concrete surface. (The presentation-sink trait is
   deferred to Phase 3.)
5. **Reshape `gecko_app` into lib + bins + App seam** — the run-loop layer; mostly relocation
   once 1–4 are stable.

---

## Step 1 — `RenderTarget` data type + resolve (`gecko_rhi`)

### Rationale
"Targets are data; sinks consume finished textures." A render target is a plain description of
where a pass's pixels land, created against RHI textures and consumed by a pass as an imported
resource. `ARCHITECTURE.md` lists render-target data under the RHI layer. This is the single
type that both the surface path and the editor-viewport path become instances of.

### Current state
- No target type exists. `SceneRenderer::render` takes two loose views:
  `color_view: &wgpu::TextureView, depth_view: &wgpu::TextureView`
  (`crates/gecko_renderer/src/scene_renderer.rs:219-229`).
- The editor's `Viewport` creates raw textures directly, outside the registry
  (`crates/gecko_editor/src/viewport.rs:16-52`), duplicating the color/depth usage flags and
  the `DEPTH_FORMAT` wiring.
- The registry stores `TextureResource { texture, view, format, size, usage }` and exposes
  `texture`, `texture_view`, `create_texture`, `replace_texture`
  (`crates/gecko_rhi/src/resource.rs:9-80`).

### Deliverables
1.1 New module `crates/gecko_rhi/src/target.rs`, re-exported from `lib.rs`, defining
`RenderTarget` and a borrowed `ResolvedTarget`.
1.2 A resolve step turning a `RenderTarget` into concrete views for the duration of encoding.
1.3 A constructor helper that allocates a colour + depth target through the registry with the
correct usage flags and the depth convention applied, plus an in-place `replace` counterpart
built on `Rhi::replace_texture` so resizes keep handles stable and free the old textures.
1.4 Retrofit `SceneRenderer::render` to consume a `ResolvedTarget` instead of two loose views.

### Design detail

```rust
// crates/gecko_rhi/src/target.rs  (finalise at implementation)

use crate::resource::TextureHandle;

#[derive(Clone, Copy, Debug)]
pub struct RenderTarget {
    pub color: TextureHandle,
    pub depth: Option<TextureHandle>,
    pub format: wgpu::TextureFormat,      // colour format
    pub extent: wgpu::Extent3d,
    pub sample_count: u32,
}

pub struct ResolvedTarget<'a> {
    pub color: &'a wgpu::TextureView,
    pub depth: Option<&'a wgpu::TextureView>,
    pub format: wgpu::TextureFormat,
    pub extent: wgpu::Extent3d,
    pub sample_count: u32,
}
```

Resolve on the registry (borrow-based, no device needed), surfaced through `Rhi`:

```rust
// crates/gecko_rhi/src/resource.rs
impl ResourceRegistry {
    pub fn resolve_target(&self, target: &RenderTarget) -> Option<ResolvedTarget<'_>> {
        let color = self.texture_view(target.color)?;
        let depth = match target.depth {
            Some(h) => Some(self.texture_view(h)?),
            None => None,
        };
        Some(ResolvedTarget { color, depth, format: target.format,
                              extent: target.extent, sample_count: target.sample_count })
    }
}

// crates/gecko_rhi/src/lib.rs
impl Rhi {
    pub fn resolve_target(&self, target: &RenderTarget) -> Option<ResolvedTarget<'_>> {
        self.registry.resolve_target(target)
    }
}
```

Constructor helper centralising the usage flags currently hand-written in `viewport.rs`:

```rust
// crates/gecko_rhi/src/target.rs
impl RenderTarget {
    /// Colour target sampled by the UI/compositor, plus a reverse-Z depth buffer.
    pub fn color_depth(
        rhi: &mut Rhi,
        label: &str,
        extent: wgpu::Extent3d,
        color_format: wgpu::TextureFormat,
    ) -> RenderTarget {
        let color = rhi.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("{label}_color")),
            size: extent, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: color_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                 | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth = rhi.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("{label}_depth")),
            size: extent, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: crate::conventions::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        RenderTarget { color, depth: Some(depth), format: color_format,
                       extent, sample_count: 1 }
    }

    /// Recreate both textures at a new extent through `Rhi::replace_texture`
    /// (`crates/gecko_rhi/src/lib.rs:108`): handles stay stable, the registry drops the
    /// old textures, and every holder of the handles sees the new textures on the next
    /// resolve. The views change, so UI-side identities must be repointed.
    pub fn replace(&mut self, rhi: &mut Rhi, label: &str, extent: wgpu::Extent3d) {
        // Same two descriptors as `color_depth`, applied via
        // rhi.replace_texture(self.color, ..) and rhi.replace_texture(depth, ..),
        // then self.extent = extent.
    }
}
```

Note on colour usage: `TEXTURE_BINDING` is required because the UI samples the colour texture.
If the Phase 3 surface-present path later blits by `copy_texture_to_texture` rather than a
sampled draw, add `COPY_SRC` here; the sampled-draw blit needs only `TEXTURE_BINDING`.

Scene renderer retrofit:

```rust
// crates/gecko_renderer/src/scene_renderer.rs  (replace color_view/depth_view params)
pub fn render(
    &self,
    rhi: &Rhi,
    encoder: &mut wgpu::CommandEncoder,
    frame_uniform_bind_group: &wgpu::BindGroup,
    target: &ResolvedTarget<'_>,
    view_proj: Mat4,
    objects: &[(Mat4, [f32; 3])],
    show_grid: bool,
) { /* use target.color / target.depth.expect("scene target has depth") */ }
```

`SceneRenderer::new(rhi, format)` is unchanged — it still needs the colour format at pipeline
construction (`scene_renderer.rs:54`). Assert or document that `target.format` matches the
format the pipeline was built with.

### Interactions
Blocks Step 2 (the offscreen target is built from `RenderTarget`). No dependency on later steps.

### Acceptance
- `RenderTarget` and `ResolvedTarget` exist in `gecko_rhi` and are re-exported.
- `SceneRenderer::render` takes a `ResolvedTarget`.
- The editor still renders: the game panel shows the scene, resize still works (the
  `Viewport` in `gecko_editor` may still own its textures at this point — it is rewired in
  Step 2). `cargo build` clean, no wgpu validation errors.

---

## Step 2 — Offscreen target ring into the RHI, single-slot (`gecko_rhi` + `gecko_editor`)

### Rationale
The editor's game image becomes a first-class render target owned below the UI, backed by
registry handles. The prior UI-layer-owned version is retired. The ring type is introduced here
but instantiated single-slot, so the ownership flip is a self-contained change before
multi-slotting in Step 3. It lives in `gecko_rhi` (deviation #2 above): it is pure GPU-resource
management with no renderer dependencies.

### Current state
- `crates/gecko_editor/src/viewport.rs` — `Viewport` owns raw `wgpu::Texture` colour + depth,
  the imgui `TextureId`, and `size`/`desired`. `apply_resize` recreates the textures and calls
  `renderer.update_external_texture_view` (`viewport.rs:75-91`). `Viewport::new` registers the
  colour view with imgui via `renderer.register_external_texture` (`viewport.rs:58`).
- `crates/gecko_app/src/lib.rs` — `EngineState` reaches `self.editor.viewport.color_view` /
  `.depth_view` / `.aspect()` to drive the scene pass (`lib.rs:114-125`), and
  `editor.begin_frame_maintenance` applies the resize before render (`lib.rs:91`,
  `crates/gecko_editor/src/lib.rs:139-141`).
- The game panel is drawn in `Editor::render` with `ui.image(self.viewport.texture_id, ...)`,
  and the desired panel size is captured there into `self.viewport.desired`
  (`crates/gecko_editor/src/lib.rs:154, 182-202`).

### Deliverables
2.1 New type `RenderTargetRing` in `crates/gecko_rhi/src/target.rs`, owning `Vec<RenderTarget>`
built via `RenderTarget::color_depth`, plus `size`/`desired` and colour format. Instantiated
with `slot_count = 1` in this step.
2.2 `EngineState` (in `gecko_app`) owns the `RenderTargetRing`; the scene pass renders into its
resolved slot 0.
2.3 `gecko_editor` retains only the imgui-side identity: a `TextureId` and helpers to register
and update it. Delete the texture-owning parts of `viewport.rs`.
2.4 The desired game-panel size flows editor → app so the app can resize the ring.

### Design detail

```rust
// crates/gecko_rhi/src/target.rs  (finalise at implementation)
pub struct RenderTargetRing {
    slots: Vec<RenderTarget>,
    size: (u32, u32),
    desired: (u32, u32),
    color_format: wgpu::TextureFormat,
}

impl RenderTargetRing {
    pub fn new(rhi: &mut Rhi, label: &str, color_format: wgpu::TextureFormat,
               size: (u32, u32), slot_count: usize) -> Self {
        let extent = extent_of(size);
        let slots = (0..slot_count)
            .map(|i| RenderTarget::color_depth(rhi, &format!("{label}[{i}]"), extent, color_format))
            .collect();
        Self { slots, size, desired: size, color_format }
    }

    pub fn slot(&self, i: usize) -> &RenderTarget { &self.slots[i] }
    pub fn color_handle(&self, i: usize) -> TextureHandle { self.slots[i].color }
    pub fn slot_count(&self) -> usize { self.slots.len() }
    pub fn size(&self) -> (u32, u32) { self.size }
    pub fn set_desired(&mut self, size: (u32, u32)) { self.desired = size; }

    /// Resize all slots in place on size change. Returns `true` when resized, so the
    /// caller repoints the UI-side identity(ies) — handles stay stable but views change.
    pub fn apply_resize(&mut self, rhi: &mut Rhi, label: &str) -> bool {
        if self.desired == self.size || self.desired.0 == 0 || self.desired.1 == 0 {
            return false;
        }
        let extent = extent_of(self.desired);
        for (i, slot) in self.slots.iter_mut().enumerate() {
            slot.replace(rhi, &format!("{label}[{i}]"), extent);
        }
        self.size = self.desired;
        true
    }
}
```

`apply_resize` must go through `RenderTarget::replace` (`Rhi::replace_texture`), not fresh
`color_depth` targets: building new targets would strand the old handles in the registry and
leak a colour + depth texture on every panel resize — nothing destroys registry entries
implicitly.

No `aspect()` on the ring. Aspect ratio is a `View` concern (Phase 4): the projection is
finalised at view time from the target extent. For Phase 2, compute it at the call site from
`ring.size()` — treat this as interim.

Editor side — replace `Viewport` with a thin identity holder (single id this step):

```rust
// crates/gecko_editor/src/viewport.rs  (reduced) or fold into lib.rs
use dear_imgui_rs::TextureId;
use dear_imgui_wgpu::WgpuRenderer;

pub struct GameImage {
    pub texture_id: TextureId,
    pub panel_size: (u32, u32),   // desired size read from the imgui content region
}

impl GameImage {
    pub fn new(renderer: &mut WgpuRenderer, texture: &wgpu::Texture, view: &wgpu::TextureView) -> Self {
        Self { texture_id: renderer.register_external_texture(texture, view), panel_size: (1, 1) }
    }
    pub fn repoint(&mut self, renderer: &mut WgpuRenderer, view: &wgpu::TextureView) {
        renderer.update_external_texture_view(self.texture_id, view);
    }
}
```

(`register_external_texture` takes `(&wgpu::Texture, &wgpu::TextureView)` — `viewport.rs:58`.
Obtain both from the registry: `rhi.registry().texture(handle)` gives the `TextureResource` with
`.texture` and `.view`.)

Wiring in `gecko_app::EngineState`:

- Construct: `let game_ring = RenderTargetRing::new(&mut rhi, "game", surface.format(), (1280, 720), 1);`
  then register with the editor using slot 0's texture + view.
- Maintenance (before render, replacing `editor.begin_frame_maintenance` at `lib.rs:91`):
  ```rust
  game_ring.set_desired(editor.game_panel_size());
  if game_ring.apply_resize(&mut rhi, "game") {
      editor.repoint_game_image(&rhi, game_ring.color_handle(0));
  }
  ```
- Scene pass (replacing `lib.rs:114-125`): resolve slot 0 and pass it in:
  ```rust
  let resolved = rhi.resolve_target(game_ring.slot(0)).expect("game slot");
  let (w, h) = game_ring.size();
  let view_proj = scene.camera.proj(w as f32 / h.max(1) as f32) * scene.camera.view();
  scene_renderer.render(&rhi, &mut encoder, frame.frame_uniform_bind_group(),
                        &resolved, view_proj, &scene.draw_list(), scene.show_grid);
  ```
- `Editor::render` no longer computes into `viewport.desired`; it writes `game_panel_size`
  and draws `ui.image(self.game_image.texture_id, ...)`. Expose
  `Editor::game_panel_size(&self) -> (u32, u32)`.

### Interactions
Depends on Step 1. Feeds Step 3 (multi-slot). The ring *type* is in rhi; the *instance* is owned
by the App assembling the frame (`EngineState` now, `EditorApp` after Step 5).

### Acceptance
- No raw `wgpu::Texture` for the game image remains in `gecko_editor`; the colour/depth
  textures live in the registry, referenced by the ring's handles.
- The editor game panel renders and resizes correctly; aspect ratio derived from the ring size,
  no validation errors.

---

## Step 3 — Multi-slot the ring (`gecko_rhi` + `gecko_editor`)

### Rationale
Size the ring to `frames_in_flight` so the main-thread UI can sample a completed slot while the
next is produced. This is the concrete cross-thread handoff object referenced throughout the
plan; it is built now even though rendering is still single-threaded, because retrofitting it
later is the expensive path.

### Current state
- After Step 2, `RenderTargetRing` exists with one slot.
- The frame ring already exists: `FramesInFlight` with `slot_index()` and `frame_index()`
  (`crates/gecko_rhi/src/frame.rs:37-98`); `FrameContext` exposes `slot_index`
  (`frame.rs:106-113`). Ring depth is `ContextConfig::frames_in_flight` (default 2,
  `crates/gecko_rhi/src/context.rs:22`), surfaced as `Rhi::frames_in_flight()`
  (`crates/gecko_rhi/src/lib.rs:67-70`).

### Deliverables
3.1 Instantiate the ring with `slot_count = rhi.frames_in_flight().get()`.
3.2 Each slot gets a **stable** imgui identity: register one `TextureId` per slot once; on
resize, repoint each slot's identity with `update_external_texture_view` (identity stays
stable, view changes).
3.3 Per frame, the scene renders into `slot(frame.slot_index)`; the editor draws
`texture_ids[frame.slot_index]`.
3.4 Resize recreates all slots (already handled by `apply_resize`) and repoints all identities.

### Design detail

The ring type from Step 2 already supports N slots. Only the instantiation and the editor's
identity table change.

Editor side holds a `Vec<TextureId>`, one per slot, and an active index:

```rust
// crates/gecko_editor
pub struct GameImageRing {
    texture_ids: Vec<TextureId>,   // stable per slot
    active: usize,                  // set each frame to frame.slot_index
}
impl GameImageRing {
    pub fn register(renderer: &mut WgpuRenderer,
                    slots: &[(&wgpu::Texture, &wgpu::TextureView)]) -> Self { /* one id per slot */ }
    pub fn repoint_all(&self, renderer: &mut WgpuRenderer, views: &[&wgpu::TextureView]) { /* update each id */ }
    pub fn set_active(&mut self, slot: usize) { self.active = slot; }
    pub fn active_id(&self) -> TextureId { self.texture_ids[self.active] }
}
```

Per-frame flow in `EngineState::redraw`:

```rust
// maintenance — needs &mut Rhi, so it runs before begin_frame
game_ring.set_desired(editor.game_panel_size());
if game_ring.apply_resize(&mut rhi, "game") {
    editor.repoint_game_ring(&rhi, &game_ring);   // repoint all slot identities
}
// frame — the FrameContext borrows the Rhi, no &mut Rhi from here on
let mut frame = rhi.begin_frame(timing);
let slot = frame.slot_index;
editor.set_active_game_slot(slot);
// scene pass into the active slot
let resolved = rhi.resolve_target(game_ring.slot(slot)).expect("game slot");
scene_renderer.render(&rhi, &mut encoder, frame.frame_uniform_bind_group(),
                      &resolved, view_proj, &scene.draw_list(), scene.show_grid);
// editor draws ui.image(game_image_ring.active_id(), ...)
```

Notes:
- Ring depth = `rhi.frames_in_flight().get()`. Do not hardcode 2 or 3. No new slot accessor is
  needed: the slot only matters at render time, so read `frame.slot_index` after `begin_frame`.
  All resource mutation (`apply_resize`) must precede `begin_frame` — `FrameContext` borrows
  the `Rhi` for its lifetime (`crates/gecko_rhi/src/frame.rs:111`), so no `&mut Rhi` exists
  while a frame is live.
- In the current single-threaded loop the scene is rendered into the same slot the UI samples
  in the same frame; that is correct now. The previous-completed-slot read only becomes
  meaningful once the render thread lands (Phase 6). The value here is that the ring object and
  the per-slot stable identity exist so that change is mechanical.
- Keep the per-slot `TextureId` stable across resize (repoint the view, do not
  register/unregister) so imgui docking state and the `ui.image` call site never see a
  changing id.

### Interactions
Depends on Step 2. The multi-slot ring is the concrete handoff object the Phase 3 composite/sink
work and the Phase 6 render thread build on.

### Acceptance
- The game target has `frames_in_flight` slots, each with a stable imgui identity.
- The scene renders into `slot_index` each frame and the UI samples the same slot; resize
  recreates and repoints all slots. No flicker, no validation errors, `cargo build` clean.

---

## Step 4 — Surface into RHI (`gecko_rhi` + `gecko_editor`)

### Rationale
The surface primitive owns a swapchain surface and its configuration; it lives in the RHI
because it touches backend types. In a fully realised design it is reached only through a sink —
but the sink abstraction is deferred to Phase 3 (see the note below), so in Phase 2 the surface
is a concrete primitive that the run loop drives directly.

### Current state
- `crates/gecko_renderer/src/surface.rs` — `Surface { surface, config, device, queue }`.
  `Surface::new(rhi, surface, w, h)` **clones** device and queue from the RHI
  (`surface.rs:19-50`). `acquire_frame`, `present`, `resize`, `reconfigure`, `format` live
  here. `Frame::Ready(SurfaceTexture, bool) | Skip` at `surface.rs:4-9`. The swapchain configures
  with `desired_maximum_frame_latency: 2` (`surface.rs:39`) — wgpu already multi-buffers the
  swapchain, so the surface path needs no ring of its own.
- Adapter selection uses the primary window surface as `compatible_surface`
  (`crates/gecko_rhi/src/context.rs:51-56`). `Rhi::new` returns
  `(Self, wgpu::Surface<'static>)` (`crates/gecko_rhi/src/lib.rs:43-63`); `EngineState::new`
  wraps it with `Surface::new` (`crates/gecko_app/src/lib.rs:44-47`).
- Present is inline in `redraw`: `surface.acquire_frame()` → build encoder → game pass →
  editor UI pass into the surface view → `frame.submit`/`frame.end` → `surface.present`
  (`crates/gecko_app/src/lib.rs:95-165`). The editor's imgui draws directly into the acquired
  swapchain view (`lib.rs:128-160`).

### Deliverables
4.1 Keep the primary-surface coupling for adapter selection; return the primary as a `Surface`
primitive and add creation of secondary surfaces.
4.2 Move the `Surface` primitive from `gecko_renderer` to `gecko_rhi`; stop cloning device and
queue — take them per call from the context.
4.3 Route the editor's present through the concrete surface (loop-driven), replacing the inline
logic.

> **Deferred: `PresentationSink`.** In Phase 2 the surface and the offscreen target are not
> symmetric. The offscreen target is *consumed* (the UI samples it); the editor surface is
> *written to* (imgui renders straight into the acquired swapchain view — `lib.rs:128-160`).
> There is no shared "consume a finished texture" shape, and the only would-be consumer
> (viewport handoff) reduces to selecting a slot index — three lines, already done in Step 3.
> The trait earns its keep in Phase 3, when the standalone final-composite blit and (later) a
> capture sink give it genuinely symmetric consumers. So Phase 2 ships a **concrete** `Surface`
> and a **concrete** slot-selection handoff; no trait. The write path is already
> consumer-agnostic without it (it renders to a `ResolvedTarget`).

### Design detail

**4.1 Adapter selection coupling (keep it).** The primary window surface remains the
`compatible_surface` for adapter selection — this picks an adapter that can actually present
and the optimal one on multi-GPU systems. Do **not** switch to `compatible_surface: None`.

- `Context::new` is unchanged internally: it creates the primary surface and selects the
  adapter with `compatible_surface: Some(&primary)` (`context.rs:49-56`), returning the raw
  primary surface.
- `Rhi::new` wraps the returned raw primary into a `Surface` primitive and returns
  `(Rhi, Surface)`. The primary `Surface` is returned **unconfigured**; the caller configures
  it by calling `resize` once with the real window size (the RHI does not depend on winit and
  should not learn the initial extent at construction).
- Add `Rhi::create_surface` for secondary windows, built against the already-chosen instance
  and adapter — no re-selection:

```rust
// crates/gecko_rhi/src/lib.rs
impl Rhi {
    pub fn create_surface(
        &self,
        surface_target: impl Into<wgpu::SurfaceTarget<'static>>,
    ) -> Result<Surface, RhiError> {
        let raw = self.context.instance().create_surface(surface_target)?;
        // Fail loud if the chosen adapter cannot present to this window (rare: a secondary
        // window on a GPU the selected adapter cannot drive).
        let caps = raw.get_capabilities(self.context.adapter());
        if caps.formats.is_empty() {
            return Err(RhiError::SurfaceUnsupported);
        }
        Ok(Surface::new(raw, self.context.adapter()))   // unconfigured
    }
}
```

Add `RhiError::SurfaceUnsupported` (and, if used above, keep `CreateSurface` which already
exists at `lib.rs:16-17`).

**4.2 Surface primitive in RHI, borrow device per call.** New
`crates/gecko_rhi/src/surface.rs`; delete `crates/gecko_renderer/src/surface.rs` and its
`pub mod surface;` (`crates/gecko_renderer/src/lib.rs:2`). The primitive stores no device or
queue — they are passed in from the context at each call:

```rust
// crates/gecko_rhi/src/surface.rs  (finalise at implementation)
pub enum Frame {
    Ready(wgpu::SurfaceTexture, bool),  // bool = reconfigure after present
    Skip,
}

pub struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    configured: bool,
}

impl Surface {
    pub(crate) fn new(surface: wgpu::Surface<'static>, adapter: &wgpu::Adapter) -> Self {
        // Format selection as today (surface.rs:24-28): prefer Bgra8UnormSrgb / Rgba8UnormSrgb.
        // Build config with width/height = 1, present Fifo, usage RENDER_ATTACHMENT.
        // configured = false until resize() is called.
    }
    pub fn format(&self) -> wgpu::TextureFormat { self.config.format }
    // width/height kept: redraw reads them for the imgui framebuffer size (lib.rs:157-158).
    pub fn width(&self) -> u32 { self.config.width }
    pub fn height(&self) -> u32 { self.config.height }
    pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) { /* set + configure */ }
    pub fn reconfigure(&self, device: &wgpu::Device) { /* configure */ }
    pub fn acquire(&mut self, device: &wgpu::Device) -> anyhow::Result<Frame> { /* as surface.rs:83-97 */ }
    pub fn present(&self, queue: &wgpu::Queue, frame: wgpu::SurfaceTexture, reconfigure: bool,
                   device: &wgpu::Device) { /* queue.present, reconfigure if needed */ }
}
```

Add convenience wrappers on `Rhi` so the surface usage stays a one-liner and the context
accessors remain `pub(crate)`:

```rust
// crates/gecko_rhi/src/lib.rs
impl Rhi {
    pub fn acquire(&self, surface: &mut Surface) -> anyhow::Result<Frame> {
        surface.acquire(self.context.device())
    }
    pub fn present(&self, surface: &Surface, frame: wgpu::SurfaceTexture, reconfigure: bool) {
        surface.present(self.context.queue(), frame, reconfigure, self.context.device());
    }
    pub fn resize_surface(&self, surface: &mut Surface, w: u32, h: u32) {
        surface.resize(self.context.device(), w, h);
    }
}
```

Consequence: `gecko_editor` and `gecko_app` stop importing `gecko_renderer::surface`. The
editor's `Editor::new` takes the surface **format** (it currently takes `&Surface` only to read
`surface.format()` at `crates/gecko_editor/src/lib.rs:52, 76`; the third read at `lib.rs:86`
is deleted with the `Viewport` in Step 2); change the parameter to
`format: wgpu::TextureFormat`.

**4.3 Route present through the surface.** Replace the inline `surface.acquire_frame()` /
`surface.present()` in `redraw` (`lib.rs:95-98, 165`) with the RHI wrappers:

```rust
let (surface_texture, reconfigure) = match rhi.acquire(&mut surface)? {
    Frame::Ready(tex, reconf) => (tex, reconf),
    Frame::Skip => return Ok(()),
};
let surface_view = surface_texture.texture.create_view(&Default::default());
// begin_frame → scene pass into game ring slot → editor imgui into surface_view → frame.end
rhi.present(&surface, surface_texture, reconfigure);
```

### Interactions
Depends on Steps 1–3. Surface relocation removes `gecko_renderer`'s device/queue clone usage.
Sets up Step 5, where the surface and window are owned together by the run loop.

### Acceptance
- `gecko_renderer::surface` no longer exists; `Surface` lives in `gecko_rhi` and stores no
  device/queue.
- Adapter selection still uses the primary surface; `Rhi::create_surface` exists for
  secondaries and rejects an unpresentable surface with a typed error.
- The editor acquires and presents through the concrete surface (via `Rhi` wrappers); the game
  image is handed off by slot selection (Step 3). `cargo build` clean, no validation errors.
- No `PresentationSink` trait is introduced (deferred to Phase 3).

---

## Step 5 — Reshape `gecko_app` into lib + bins + `App` seam (`gecko_app` + `gecko_editor`)

### Rationale
Window management — tracking OS windows and their surface primitives, and mapping window
events onto surface reconfiguration — belongs in the run-loop layer, not the renderer. Realised
here as `gecko_app` reshaped into a library plus thin binaries, with an `App` seam so the loop
never names imgui directly. This is the largest structural move and comes last because it is
mostly relocation of now-stable pieces.

### Current state
- `crates/gecko_app/src/main.rs` — `fn main() { gecko_app::run() }`.
- `crates/gecko_app/src/lib.rs` — `EngineState` owns `window, rhi, surface, editor, scene,
  scene_renderer` plus timing (`lib.rs:20-32`); `redraw` runs the whole frame
  (`lib.rs:71-170`); `App` implements `ApplicationHandler` (`lib.rs:184-239`); `run` builds the
  event loop (`lib.rs:241-259`). Multi-viewport is behind `#[cfg(feature = "multi-viewport")]`
  and needs the event loop threaded per frame via `gecko_editor::set_event_loop_for_frame`
  (`lib.rs:222-223`, `crates/gecko_editor/src/lib.rs:26`).
- `crates/gecko_app/Cargo.toml` — `default = ["multi-viewport"]`; `multi-viewport =
  ["gecko_editor/multi-viewport"]`; unconditional dependency on `gecko_editor`.

### Deliverables
5.1 Define an `App` trait (the editor-agnostic seam) and an `EditorApp` in `gecko_app`
(feature-gated) that composes `gecko_editor::Editor` with the render-side state and
implements `App`.
5.2 Move the run-loop state and event handling into `gecko_app`'s library, generic over `App`.
5.3 Relocate the "what to render" state — `Scene`, `SceneRenderer`, the `RenderTargetRing`
instance — into `EditorApp`. `gecko_editor` stays a library of editor UI (panels, the imgui
bridge, the identity table) with no application loop or `App` impl.
5.4 Convert `gecko_app` to library + `src/bin/editor.rs`; feature-gate the editor dependency so
a future game binary does not drag imgui.
5.5 Window management: the loop owns windows and their surfaces and maps resize/close/DPI onto
surface reconfiguration; structure so multiple windows are multiple entries and the imgui
multi-viewport platform backend coexists.

### Design detail

**5.1 The `App` seam.** The loop owns window(s), surface(s), the RHI, timing, and phase
sequencing. The `App` owns everything above: scene, renderer state, the render-target ring
instance, and (for the editor) imgui. Proposed trait _(finalise at implementation)_:

```rust
// crates/gecko_app/src/lib.rs
use std::sync::Arc;
use gecko_rhi::{Rhi, frame::FrameContext};
use winit::{event::WindowEvent, window::{Window, WindowId}};

pub struct AppInit<'a> {
    pub rhi: &'a mut Rhi,
    pub primary_format: wgpu::TextureFormat,
    pub window: &'a Arc<Window>,
}

pub struct RenderCtx<'a, 'f> {
    pub rhi: &'a Rhi,                          // shared — a live FrameContext borrows the Rhi
    pub frame: &'a mut FrameContext<'f>,
    pub surface_view: &'a wgpu::TextureView,   // acquired swapchain view
    pub surface_size: (u32, u32),
    pub dt: f32,
}

pub trait App: Sized {
    fn new(init: AppInit<'_>) -> anyhow::Result<Self>;
    fn on_event(&mut self, window: &Arc<Window>, id: WindowId, event: &WindowEvent);
    fn update(&mut self, dt: f32);

    /// Pre-frame resource maintenance (ring resize, identity repoints). The only hook
    /// with `&mut Rhi`; runs before `begin_frame`. Default no-op.
    fn maintain(&mut self, _rhi: &mut Rhi) {}

    fn render(&mut self, ctx: RenderCtx<'_, '_>) -> anyhow::Result<()>;
    fn wants_quit(&self) -> bool;

    // Editor multi-viewport hook; default no-op so a game App ignores it.
    fn end_frame(&mut self) {}
}
```

`RenderCtx` carries `&Rhi`, never `&mut Rhi`: `FrameContext` holds a shared borrow of the
`Rhi` for its whole lifetime (`crates/gecko_rhi/src/frame.rs:111`), so a mutable borrow cannot
coexist with a live frame. All resource mutation happens in `maintain`, before `begin_frame` —
the same ordering the current code uses (`begin_frame_maintenance` at `lib.rs:91` before
`begin_frame` at `lib.rs:104`). The two lifetimes are deliberate: `&'a mut FrameContext<'a>`
would tie the frame's borrow to itself and lock the caller out.

The multi-viewport specifics (`update_platform_windows`) stay inside `EditorApp` behind the
existing `#[cfg(feature = "multi-viewport")]` gates; the loop calls `app.end_frame()` after
present. The per-frame event-loop guard (`set_event_loop_for_frame`, currently `lib.rs:222-223`)
cannot live in the bin: it needs the `&ActiveEventLoop` handed to the loop's `window_event`
callback and must be held across `redraw` as an RAII guard. The loop installs it itself behind
`#[cfg(feature = "multi-viewport")]`, through the `gecko_editor::set_event_loop_for_frame`
re-export (`crates/gecko_editor/src/lib.rs:26`) — the one feature-gated place the loop names the
editor crate; everything else stays behind the `App` seam.

**5.2 Generic run loop.** `EngineState<A: App>` owns `window`, `rhi`, the primary `Surface`,
timing, and `app: A`. The existing `struct App` (`lib.rs:179-182`) — the winit
`ApplicationHandler` shim holding `Option<EngineState>` — is renamed (its name now belongs to
the trait) and made generic over `A`; its deferred construction in `resumed` stays. `redraw`
becomes:

```rust
fn redraw(&mut self) -> anyhow::Result<()> {
    let dt = /* timing as lib.rs:74-89 */;
    self.app.update(dt);
    self.app.maintain(&mut self.rhi);   // last &mut Rhi access this frame
    let (tex, reconf) = match self.rhi.acquire(&mut self.surface)? {
        Frame::Ready(tex, reconf) => (tex, reconf),
        Frame::Skip => return Ok(()),
    };
    let view = tex.texture.create_view(&Default::default());
    let mut frame = self.rhi.begin_frame(FrameTiming { time, delta_time: dt });
    self.app.render(RenderCtx { rhi: &self.rhi, frame: &mut frame,
                                surface_view: &view, surface_size: self.surface_size, dt })?;
    frame.end();
    self.rhi.present(&self.surface, tex, reconf);
    self.app.end_frame();
    Ok(())
}
```

**5.3 Relocate render state into `EditorApp`.** A feature-gated `EditorApp` in `gecko_app`
(e.g. `src/editor_app.rs`) composes `gecko_editor::Editor` with `Scene`, `SceneRenderer`, and
the `RenderTargetRing` instance, and implements `App`. `gecko_editor` stays a library of editor
UI — panels, the imgui bridge, the `GameImageRing` identity table (it wraps the imgui
renderer) — with no application loop or `App` impl. `EditorApp::maintain` performs the
pre-frame work (`ring.set_desired` / `apply_resize` / repoint — the only point with
`&mut Rhi`); `EditorApp::render` runs the scene pass into the active ring slot, then the
editor's imgui into `ctx.surface_view`; the panel code only reads `active_id()`. This
consolidates ownership: the App owns "what to render," the loop owns "window and frame," and
the editor crate owns "how it looks." (This supersedes Step 2/3's interim ownership in
`EngineState`. The ring *type* stays in rhi; only the *instance* moves.)

**5.4 Crate layout.** Convert `gecko_app` to a library plus a binary:

```
crates/gecko_app/
  src/lib.rs          # App trait, EngineState<A>, run<A>(), window + event handling
  src/editor_app.rs   # #[cfg(feature = "editor")] EditorApp: Editor + Scene + SceneRenderer + ring
  src/bin/editor.rs   # fn main() { gecko_app::run::<gecko_app::EditorApp>() }
  # src/bin/game.rs   # later
```

Delete `src/main.rs`. `Cargo.toml`:

```toml
[features]
default = ["editor"]
editor = ["dep:gecko_editor", "multi-viewport"]
multi-viewport = ["gecko_editor?/multi-viewport"]
tracy = ["gecko_core/tracy"]

[dependencies]
gecko_editor = { workspace = true, optional = true }
# gecko_renderer / gecko_rhi / gecko_runtime / gecko_core remain non-optional

[[bin]]
name = "editor"
required-features = ["editor"]
```

`required-features` makes Cargo skip the editor bin when the feature is absent, so a future
`game` bin can build with `--no-default-features` and never compile imgui.

`run::<A>()` (generic over the app) replaces the concrete `run()` at `lib.rs:241-259`; the
editor bin instantiates it with `EditorApp` as `A`.

**5.5 Window management.** The loop owns each window and its surface together, and maps events
to surface reconfiguration — the logic currently at `lib.rs:214-235`:

- `WindowEvent::Resized` / `ScaleFactorChanged` → `rhi.resize_surface(&mut surface, w, h)`.
- `WindowEvent::CloseRequested` on the primary → exit.
- Other windows: hold a table keyed by `WindowId` mapping to `(Window, Surface)`. Multiple
  game/editor windows are simply multiple entries here; each is fed by one or more views in a
  later phase. For Phase 2 a single primary window is sufficient — the structure must merely
  admit more without redesign.
- Keep the imgui multi-viewport windows separate: they are spawned by imgui through the winit
  platform backend (`crates/gecko_editor/src/lib.rs:100-115, 207-214`). The loop does not
  manage those; it only forwards events through `App::on_event` (which `EditorApp` routes to
  `Editor::handle_window_event`) and calls `app.end_frame()`. Do not let the imgui window
  model dictate the loop's window table.

### Interactions
Depends on Step 4 (surface in RHI). Final step; nothing in Phase 2 depends on it.

### Acceptance
- `gecko_app` is a library with `src/bin/editor.rs`; `cargo build --bin editor` works;
  `cargo build --no-default-features` builds the library without compiling `gecko_editor` /
  imgui.
- The run loop is generic over `App`; `gecko_editor` is named only in the feature-gated
  `EditorApp` module, the editor bin, and the feature-gated multi-viewport guard.
- Window resize/close/DPI drive surface reconfiguration from the loop; multi-viewport still
  works under the `multi-viewport` feature.
- The editor runs exactly as before from the user's perspective: scene in the game panel,
  docking, console, inspector, hierarchy, resize — no validation errors.

---

## Cross-cutting conventions

- **No device clones above the RHI** except the sanctioned imgui-init clones and
  `SceneRenderer::new`'s pipeline construction (until the Phase 5 pipeline cache). Step 4
  removes the renderer surface's clones; do not reintroduce them.
- **All GPU textures via the registry.** After Step 2 there should be no raw `wgpu::Texture`
  owned above `gecko_rhi` except swapchain textures (which are transient and owned by the
  surface).
- **Depth via `conventions`.** Never hardcode `Depth32Float`, `Greater`, or `0.0` for depth.
- **Ring depth from `Rhi::frames_in_flight()`.** Never hardcode the slot count.
- **Consumer-agnostic write path.** The scene pass renders to a `ResolvedTarget` and never
  names its consumer.
- **Typed errors.** New failure modes (unpresentable secondary surface, missing target
  resolve) return typed `RhiError` variants with actionable messages, not panics or silent
  `false` returns, unless matching an existing pattern in the registry.

## Global verification checklist

Run after each step (per the project verification rules):

- `cargo build` — clean.
- `cargo clippy` — clean under the workspace lints (`Cargo.toml` `[workspace.lints.clippy]`).
- `cargo fmt --check` — formatting passes.
- Run the editor: scene renders in the Game panel, orbit camera controls work, resizing the
  Game panel and the window both work, docking/console/inspector/hierarchy intact, no wgpu
  validation errors in the log.

## Out of scope — deferred to Phase 3+

- **`PresentationSink` trait.** Deferred to Phase 3. In Phase 2 the surface and the offscreen
  target are not symmetric (the editor UI writes the swapchain directly; the offscreen target is
  consumed by the UI), so the abstraction would have one trivial consumer and no shared shape.
  It lands with the composite step, where the standalone final-composite blit and a capture sink
  give it real, symmetric consumers.
- **Pass abstraction and the explicit pass sequence** — the frame is still one hardwired
  encode in `redraw`. Passes producing independent command buffers come in Phase 3.
- **The final-composite feature and the surface-present blit** — Phase 3, with a standalone
  binary to exercise it.
- **Views, view scheduler, render paths** — Phase 4. Aspect ratio stays computed at the call
  site from the ring size for now; it moves to the view later.
- **Capture sink (screenshot/video)** — a further consumer, added when needed (with the sink
  trait, Phase 3+).
- **The render thread** — Phase 6. The ring and the surface seam built here are what make it a
  mechanical addition later.
- **A dedicated `gecko_run_loop` crate** — promote `gecko_app`'s library out into its own
  crate only once a second binary makes the editor-agnostic boundary worth compiler
  enforcement.
