# Renderer Architecture — Phased Implementation Plan

A migration plan for building a layered, scalable renderer on top of wgpu. It takes
the layered abstraction design as its target and sequences the work into phases,
front-loading the decisions that are cheap now and painful to retrofit, and deferring
the machinery that only earns its cost at scale.

The plan is shaped by two facts specific to this engine that reorder the clean
bottom-to-top layer sequence:

- The immediate-mode UI (dear-imgui) owns the window surface in the editor, where the
  game is an offscreen texture the UI samples; in a standalone game build, a final
  composite owns the surface and the UI is at most an overlay. **Who writes the
  swapchain is therefore per-binary**, which forces the frame into a two-step shape —
  scene encode to an offscreen texture, then composite to a surface — with the
  offscreen texture in between.
- That same gap between encode and composite is where a render thread will later
  split (encode off the main thread, composite and present on it, because the UI
  library's main-thread affinity is non-negotiable). The offscreen texture is thus a
  cross-thread handoff and must be ring-buffered and owned below the UI from day one.

One seam serves both concerns, so the phases are ordered to build it early rather than
retrofit it.

Names below are deliberately generic. Everything is referred to by its role, not by any
specific type, file, or crate, so the plan maps onto the real code regardless of exact
current naming.

---

## Architecture Overview — Target State

wgpu is the real hardware-abstraction layer. It already solves the problems that
dominate raw Vulkan/D3D12 engine code — resource lifetime tracking, barriers and layout
transitions, staged uploads — and this design does **not** re-solve any of them. The
layers we add are organizational: they address structure at engine scale, which lets
each component stay far smaller than its lower-level namesake.

**Guiding principles**

1. **Native desktop target.** There is no web/WebGPU build — the scripting runtime rules it
   out — so the plan assumes native capabilities throughout: more than four bind groups where
   hardware allows, push constants, and bindless for later. This gates several of the decisions
   below and is specified in Phase 0.
2. **A thin engine RHI over wgpu, isolated by capability rather than token-avoidance.** One
   layer — its own crate — owns the device, queue, and all live resources and is the only code
   that issues GPU operations. Everything above holds handles and cannot obtain a device, so it
   cannot create resources or encode outside a pass; it may still freely *name* wgpu's plain
   data types. Making the RHI a crate rather than a module puts that boundary under the compiler
   instead of under review.
3. **Resources are referenced by opaque handles.** Handles exist for *engine* reasons —
   serialization, stable identity across hot reload, feeding opaque IDs to the scripting
   runtime, isolating upper layers from the backend — not for lifetime safety, which wgpu
   already provides.
4. **Targets are data; sinks consume finished textures.** The renderer never draws
   directly to a swapchain. All rendering lands in ordinary offscreen textures. A render
   target is a plain description of where pixels go; a surface, an editor viewport, and a
   screenshot are interchangeable sinks for a finished texture. This unifies game windows,
   editor viewports, and capture into one path and makes multiple windows fall out.
5. **A frame is a set of views scheduled into a pass sequence.** A view binds a camera and
   viewport to a target and a render recipe. The main game view, each editor viewport, each
   shadow cascade, and each reflection probe are the same concept scheduled the same way.
6. **Explicit pass sequence now; full frame graph later.** Passes are authored as an
   ordered list that produces command buffers, with each pass's resources resolved to
   locals before it encodes. The scheduling, culling, and transient-pooling machinery of a
   true frame graph is deferred until pass count justifies it — but the authoring surface is
   graph-shaped, so the graph can slot underneath without disturbing authored features.
7. **Frequency-based binding and data-driven materials.** A fixed bind-group frequency
   contract (per-frame / per-view / per-material / per-draw) holds across the scene path, over
   an underlying rule — lowest index for lowest change-frequency — that is universal across all
   passes. Per-draw identity travels as a push constant and per-instance data as a per-view
   storage buffer, so the per-draw group stays nearly empty in the hot path. Materials are pure
   data referencing pipelines by handle; layouts are derived by shader reflection; hot reload
   works by invalidating cached pipelines. Specified in Phase 0.
8. **A frame loop with hard phase boundaries.** Input, simulate, extract, encode, composite,
   present run as explicit stages. An immutable snapshot taken at extract decouples
   simulation from rendering and is also where fixed-timestep interpolation happens.
9. **The encode-to-composite seam is the threading boundary.** It is designed in from the
   start — a ring-buffered offscreen target sits in the gap — so introducing a render thread
   later is a mechanical change, not a restructuring.

Two convention sets are fixed engine-wide and specified in full in Phase 0: the render math and
depth setup — right-handed, Y-up, infinite reverse-Z on a Depth32Float buffer — and the GPU
interface contract — the bind-group tiers, the vertex attribute locations, and the
push-constant-plus-storage-buffer instancing model.

**Organizational shape (by role, not fixed crates or modules)**

- **RHI layer** — the GPU context, the resource registry and its handles, the pipeline
  cache, the per-frame context and frames-in-flight ring, render-target data, and the
  surface primitive. Its own crate: it owns the device, queue, and live resources and is the
  only code that issues GPU operations. Code above may name wgpu data types but cannot obtain a
  device.
- **Renderer layer** — passes, views, render paths, materials, render lists, and features.
  Depends on the RHI, strictly one-directional — and because the RHI is a separate crate, that
  direction is enforced by the compiler.
- **Run-loop layer** — window management, event routing, UI-library platform integration,
  phase sequencing, and later render-thread orchestration. A library that both the editor
  and standalone-game binaries depend on; each binary's entry point becomes little more than
  window creation plus a call into it.
- **Simulation/runtime layer** — the world and the extraction of visible objects into the
  renderer's snapshot types. Never depends on rendering internals.
- **Assets layer** — CPU-side asset representation, loading, and hot-reload watching. Its
  heavy, format-specific dependencies are isolated here. It produces CPU data; the renderer
  uploads it.
- **Scripting/interop layer** — hands opaque handle IDs to the scripting runtime as plain
  values. This is the sharpest argument for handles being universal in this engine.
- **Core layer** — math, the fixed-step clock, the task pool, diagnostics, and the slotmap
  primitives handles are built on.

These are role boundaries realized as a single workspace of crates. Two are drawn as crates
from the start for specific reasons: the RHI, so its ownership rule is compiler-enforced, and
the assets layer, to isolate its heavy, format-specific dependencies. The run-loop is a library
crate both binaries depend on, and core, runtime, and interop remain their existing crates. What
deliberately stays *within* one renderer crate — as modules, not separate crates — is the
renderer's own internals: passes, views, render paths, materials, and features. Those lines are
still moving, so they are drawn as modules and promoted to crates individually only once a
public surface has stabilized.

---

## Phase 0 — Foundational Decisions (Ratified)

These decisions are locked; nothing here remains open. They are recorded because every later
phase assumes them and because changing any one of them afterward is a cross-cutting retrofit
that touches every shader, pipeline, or module boundary at once. No code is written in this
phase — it is the decision record the rest of the plan builds on.

### Target platform and capabilities
Native desktop only. There is no web/WebGPU target — the scripting runtime does not run in a
browser, so that build was never on the table — and the plan takes the native capabilities that
follow: up to eight bind groups where hardware allows, push constants, and binding
arrays / bindless for later use. In particular, the web baseline's four-bind-group ceiling does
not constrain us, and per-draw data travels through a push constant rather than consuming a bind
group. Feature and limit negotiation at startup validates these against the adapter (notably the
push-constant size limit).

### Bind-group frequency contract
For the scene/material path, bind-group slots have fixed meaning:

- **Group 0 — per-frame:** frame/time data (index, delta), the shared sampler set (so materials
  stop redefining common samplers), and global environment state.
- **Group 1 — per-view:** the camera block (view, projection, view-projection, and their
  inverses; camera position; viewport extent; near/far) plus a reserved jitter slot for future
  temporal AA.
- **Group 2 — per-material:** the material uniform block, its textures, and any
  material-specific samplers.
- **Group 3 — per-draw:** reserved, and nearly empty in the hot path (see the data model below).

The ordering is load-bearing: wgpu keeps a bind group bound across pipeline switches while the
layout prefix stays compatible, so the slot with the lowest change-frequency must have the
lowest index. What is *universal* across the whole engine is that ordering rule — low index =
low frequency. What is *specific to the scene path* is the tier content above; passes with no
notion of "material" or "object" (post-processing, UI, compute) define their own layouts and
only obey the ordering rule.

### Per-draw and per-instance data model
Per-draw identity travels as a **push constant**, not a per-draw bind group. Per-instance data
(model matrix, normal matrix, material index) lives in a **per-view storage buffer**, indexed
in-shader from a small push-constant base. Keep the push constant tiny — the draw/instance base
plus at most a couple of per-draw flags — and request a modest push-constant size with headroom
(128 bytes is portable across desktop; actual use is a handful of bytes), validated against the
adapter's maximum, with the range declared visible to the stages that use it (primarily vertex).
A consequence used below: there are no per-instance vertex attributes at all, because instance
data is pulled from storage by index.

### Vertex attribute layout
Fixed attribute location numbering, committed engine-wide: 0 position, 1 normal, 2 tangent,
3 uv0, 4 uv1/color. Tangents are included because full 3D needs normal mapping. Because
per-instance data comes from the storage buffer above, the vertex stream is purely mesh
attributes and carries no instance columns — which keeps every mesh, shader, and pipeline vertex
state on one simple layout.

### Coordinate system and depth
The engine targets full 3D (the earlier 2.5D framing is dropped). Conventions:

- **Right-handed, Y-up** — matches the math library and the wider Rust/wgpu ecosystem, and is
  where the current renderer already is, so this is ratification rather than migration.
- **Infinite reverse-Z depth** — near maps to 1, far to 0; the depth compare is `Greater`; the
  depth buffer clears to 0.0; the projection uses the math library's right-handed
  infinite-reverse constructor. "Infinite" removes the far plane entirely, so distant geometry
  never clips out and far-plane tuning disappears as a concern.
- **Depth32Float** — reverse-Z only pays off on a floating-point depth buffer; the few extra
  bits of memory are irrelevant on desktop.
- Clip depth is native wgpu range [0, 1], so no OpenGL-style depth-correction matrix is needed.

Every depth-consuming shader written later (fog, SSAO, linearization, position reconstruction)
must be written knowing depth is reversed.

### Backend isolation and the crate boundary
The isolation rule is stated in terms of capability and ownership, not token-avoidance: the RHI
owns the device, queue, and all live GPU resources and is the only code that issues GPU
operations outside a pass; everything above holds handles. Because upper layers cannot obtain a
device, they cannot create resources or encode work — but they may freely *name* wgpu's plain
data types (formats, enums, and similar), and those may be re-exported through the RHI's
namespace rather than re-wrapped. This keeps the isolation that matters (the registry, hot
reload, and the scripting boundary all depend on it) without rebuilding the portability layer
wgpu already provides.

This boundary is enforced structurally: **the RHI is its own crate from the start**, so upper
crates literally cannot name the device and the directionality is checked by the compiler rather
than by review. The **assets layer is also its own crate**, for dependency isolation. Everything
else — renderer, views, materials, features — stays as modules within a renderer crate until its
public surface stabilizes, at which point a boundary can be promoted to a crate individually. The
whole thing lives in one workspace.

---

## Phase 1 — RHI Foundation: Handles, Registry, Frame Plumbing

Reshape the GPU-owning code into a proper RHI and stand up the per-frame plumbing that every
later layer holds. This comes first because the handle model is the most painful retrofit on
the list, and the per-frame ring is simultaneously the cross-thread handoff introduced later.
It is also where the RHI becomes its own crate, per the Phase 0 boundary decision, so the
ownership rule is compiler-enforced from the first commit rather than retrofitted.

### GPU context
Consolidate the device, queue, adapter information, and feature/limit negotiation into a
single owned context that the rest of the RHI borrows from. It is the one source of truth for
who owns the GPU. Nothing above the RHI sees it directly. This is where the Phase 0 native
capabilities are requested and validated against the adapter — push-constant size in particular,
plus the bind-group count and any bindless features relied on later.

### Resource handles
Introduce generational indices (slotmap keys) for textures, buffers, and samplers — pipelines
join later. They are copyable, serializable, and stable across hot reload: reassigning what a
handle points to updates every material, pass, and view that references it, with no change at
the reference sites. These become the currency of every upper layer, and the same IDs are what
the scripting boundary later receives as opaque values.

### Resource registry
Slotmap-backed storage mapping handles to live GPU resources, and the *resolve step* on top
of it: at encode time, turn handles into concrete backend views and buffers for the duration
of encoding. The only engine-side lifetime concern — atomically retargeting a handle's lookup
during hot reload — lives here; wgpu already handles the rest.

### Per-frame context
The bundle handed to passes each frame — the acquired target(s), the command encoder(s), the
upload path, the frame index, and timing — so that no code threads the device, queue, or
encoder through call chains. This is the object encoding is built around.

### Frames-in-flight ring
Per-frame resource sets indexed by a small ring sized to the frame latency already permitted,
so per-frame uniforms and bind groups do not stall against in-flight frames. This is also the
mechanism that later lets the main thread read one frame's finished outputs while the next is
produced — so the threading handoff is physically present from Phase 1, even before any thread
split exists.

### Uploads — use the built-ins
Per-frame writes and ordinary asset uploads go through wgpu's existing staged write path. No
dedicated upload manager and no deferred-destruction system: wgpu already stages transfers and
already tracks in-flight resource usage, so dropping a resource while commands still reference
it is safe. Introduce a bespoke transfer path only when a concrete need — large streamed
assets, precise transfer scheduling — outgrows the built-ins.

---

## Phase 2 — Outputs, Surfaces, and the Offscreen Target as First-Class

Introduce the target/sink model and make the editor's game image a real, owned target rather
than an ad-hoc object living in the UI layer. This is the immediate pain point, and it
simultaneously builds half the threading seam and the foundation for multiple windows.

### Render target (data)
A plain description — texture handle, format, extent, sample count — of where a pass's pixels
land. No behavior. Created against RHI textures and consumed by the pass sequence as an
imported/external resource. This is the single type that both the surface path and the
editor-viewport path become instances of. Formats follow the Phase 0 conventions — depth
attachments are Depth32Float, for reverse-Z.

### Surface primitive (per window)
Owns a swapchain surface and its configuration: acquire, resize, surface-loss and
reconfiguration, and format/present-mode selection. It produces the transient presentable
texture that a surface-present sink copies into. It lives in the RHI because it touches backend
types, but it is only ever reached through a sink — never rendered to directly.

### Presentation sink
The abstraction over what consumes a finished texture. Two implementations to start:
surface-present (copy the finished texture into the acquired swapchain texture, then present)
and viewport handoff (expose the finished texture to the UI layer as a sampled image). Capture
— screenshot or video — is a further sink reading the same texture. This split, keeping
"where pixels land" (the target) separate from "what consumes them" (the sink), is what makes
game window, editor viewport, and capture genuinely one rendering path.

### Offscreen game target, ring-buffered
The editor's game image becomes a first-class render target owned in the renderer layer, backed
by a small ring of slots so the main-thread UI samples a completed slot while the next is
produced. Each slot needs a stable UI-side texture identity — either one identity registered
per slot, or a single identity re-pointed at the completed slot each frame. The prior
UI-layer-owned version is retired. This is the concrete cross-thread handoff object referenced
throughout the plan.

### Window management (run-loop layer)
Tracking open OS windows and their surface primitives, and mapping window events — resize,
close, DPI change — onto surface reconfiguration, belongs in the run-loop layer, not the
renderer. Windowing owns window creation, and the UI library's multi-viewport support creates
its own OS windows through the windowing platform backend; burying window management inside the
renderer fights both. This is the deliberate split of the design's single output layer into a
renderer-side half (target plus surface primitive) and a platform-side half (window and event
management). Multiple game or editor windows are simply multiple entries here, each fed by one
or more views.

---

## Phase 3 — Pass Sequence and the Encode/Composite Split (Deferred Graph)

Turn the single hardwired scene render into an explicit, ordered, graph-shaped pass sequence,
and make the frame's two-step shape — encode to offscreen, then composite to surface —
structural. This seam is where both the UI compositor model and the future render thread
converge, so it is built now and the heavy graph machinery is deliberately left out.

### Pass
A unit of GPU work that declares what it reads and what it writes and carries an encode step
receiving resolved resources. Authored features become these. Crucially, a pass produces
command buffers rather than mutating one shared encoder in place — that independence is exactly
what lets recording move onto another thread later without redesign. Non-scene passes (post, UI,
compute) define their own bind-group layouts but honor the Phase 0 ordering rule — lowest index
for lowest change-frequency.

### Pass sequence and interim resource ownership
An explicit ordered list of passes, run in order, with a deliberately naive owner for per-frame
intermediate textures — a plain per-frame map, not a pool. This is the deferred-graph decision
made concrete: the authoring surface is graph-shaped, but none of the compiler machinery
(scheduling, culling, transient pooling) is built yet. The interim owner is meant to be thrown
away in Phase 6.

### Resolve-before-encode discipline
The step that turns a pass's declared reads and writes into concrete local resources runs
*before* the encode closure, not inside it. This is a deliberate structural choice for Rust:
resolving into locals up front avoids threading registry borrows through the encode closure and
the borrow-checker friction that would otherwise cause. Bake it into the pass contract from the
start rather than discovering it later.

### The encode / composite / present shape
Make the frame explicitly three-staged. A scene pass writes into an offscreen target slot —
color plus a reverse-Z Depth32Float depth buffer, cleared to 0.0 with a `Greater` compare per
the Phase 0 depth convention; a composite step consumes that finished texture — in the editor,
the UI compositor samples it; in a standalone game, a final-composite feature blits it to the
surface; then present runs. The
offscreen target sits in the gap between encode and composite, which is exactly where a render
thread will later split (encode off-thread, composite and present on the main thread). One seam
absorbs both the per-binary question of who owns the swapchain and the future threading
boundary.

### First feature: final composite
The one feature that reaches the output layer. It takes the composed frame texture and writes it
into the view's target, after which the target's sink takes over. In the editor this reduces to
the UI sampling the target; in a standalone game it is an explicit blit into the swapchain sink.
Building it here validates the whole target/sink/pass path end to end with a single real
feature.

---

## Phase 4 — Views

Promote "a frame is a set of views" into a real abstraction. It is thin once passes and targets
exist, and it is the step that turns one hardcoded scene into many views sharing one
infrastructure.

### View
Data binding a camera and projection and a viewport rectangle to a target and a named render
path. It owns nothing backend-side beyond handles. The editor game panel becomes a view
rendering into the offscreen target; a standalone window becomes a view rendering into a
surface-backed target. Aspect ratio belongs here, not on the camera: the projection is finalized
at view time from the target's extent rather than baked into the camera upstream.

### View scheduler
Collects the frame's active views — windows and editor panels now, shadow-casting lights and
reflection probes later — orders them by dependency (for example, shadow views before the main
view), and instantiates each view's render path into the pass sequence. This is where
multi-window and multi-viewport composition actually happens.

### Render path
A named recipe — forward, shadow-depth-only, editor-wireframe — describing which features a view
runs and in what arrangement, so different views can run different pipelines through shared
infrastructure. Start with one real path (forward) plus whatever the editor overlay needs.

### Multi-window / multi-viewport reconciliation
Keep the UI library's own secondary-window mechanism and the engine's window management as
separate concerns: the UI owns the windows it spawns, and the engine's window management owns
game surfaces. Let the two coexist, and do not let the UI library's window model dictate engine
structure.

---

## Phase 5 — Materials, Binding, and Draw Submission

Add the data-driven material layer and the sort-and-batch submission path. Phase 0's binding
convention pays off here, and the extraction seam already exists, so most of this phase is
additive rather than a rewrite.

### Pipeline cache and pipeline handles
Cache render and compute pipelines and bind-group layouts keyed by their descriptors. Upper
layers request pipelines by description rather than constructing them, which enables permutation
reuse and shader hot reload. Pipeline handles join the handle family at this point. Scene-path
pipelines are built against the Phase 0 bind-group tiers and the per-draw push-constant range.

### Shader module registry and reflection
Load shaders, run reflection to derive bind-group layouts and vertex inputs so layouts are not
hand-maintained, and support hot reload by invalidating the dependent cached pipelines.
Reflection-derived layouts are what keep the frequency convention honest across a growing set of
shaders.

### Material template vs. instance
A template defines a material *type* — shader, permutation flags, parameter layout, blend and
depth state — and requests its pipelines from the cache. An instance is pure data — a template
reference plus parameter values and texture handles — and is what scene objects actually
reference. Both are serializable and hot-reload friendly.

### Permutation management
Manage shader feature combinations — skinned versus static, lit versus unlit, and so on — as
compile-time variants mapped to cache keys, keeping the total pipeline count bounded as features
multiply.

### Scene extraction
Formalize the per-view world walk that frustum-culls and produces render records (mesh handle,
material instance, transform) decoupled from gameplay data structures. This lives in the
simulation/runtime layer and fills the renderer's snapshot types, so the renderer never depends
on the ECS. The existing frame snapshot already sits on this boundary; this phase gives it
culling and a stable record shape.

### Render list and batching
Per view and per phase — opaque, transparent, shadow — a sorted array of draw records keyed by a
packed integer sort key (pipeline, then material, then depth): opaque sorted front-to-back to
minimize state changes, transparent back-to-front for correctness. A batch builder merges
compatible records into instanced draws, writes per-instance data through the upload path, and
emits the final draw stream a pass consumes. This is where real rendering scale — state-change
minimization and instancing — actually lives.

---

## Phase 6 — Real Graph, Feature Breadth, and the Render Thread

Only once pass count and profiling justify it: build the true frame-graph machinery, expand the
feature set, and finally introduce the render thread. Everything this phase needs was already
seam-built in Phases 1 through 3, so each item is an addition rather than a restructuring.

### Graph compiler
Dependency analysis over the declared passes: topological ordering, dead-pass culling (a
disabled effect is simply never declared), and lifetime computation for the transient pool. Keep
it separate from the pass-authoring surface so the compilation strategy can evolve — including
parallel command encoding across passes — without changing how features are authored.

### Transient resource pool
Allocate and reuse per-frame intermediate textures based on the compiled lifetimes, replacing
Phase 3's naive per-frame map. Note a wgpu-specific limit: there are no placed resources, so
true memory aliasing between non-overlapping passes is not possible — the savings come instead
from reusing whole textures across frames and between compatible passes, which is still a
substantial win for post-processing stacks and per-view intermediates.

### Feature breadth
The actual technique library, each technique authored as one or more graph passes and composed
into render paths: depth prepass, opaque, transparent, a post-processing chain (bloom,
tonemapping), and a UI pass. The post chain is the showcase for transient reuse and dead-pass
culling — disabled effects are never declared and therefore cost nothing.

### Render thread
Overlay the encode/composite split with an actual thread boundary: scene encode on a render
thread, composite and present on the main thread (the UI library's main-thread affinity is
non-negotiable), with the ring-buffered offscreen target as the handoff. Per the broader
threading plan, this comes *after* background asset I/O and data-parallel simulation, and only
once profiling shows the main thread is the bottleneck. Because Phases 1 through 3 already built
the ring, the resolve-before-encode discipline, and command-buffer-producing passes, this
becomes a mechanical addition.

---

## Sequencing Rationale

The order is driven by retrofit cost and by the two engine-specific facts, not by the layer
numbers.

**Phases 1 through 3 are the load-bearing spine.** They build three things that turn out to be
the same thing viewed from different angles: the RHI handle model, the ring-buffered offscreen
target, and the encode/composite seam. Together these deliver the target/viewport unification
that is the immediate need, the UI compositor structure, and the render-thread boundary — all at
once. The handle model leads because it is the worst retrofit; the offscreen ring is pulled
forward because it is the cross-thread handoff; the seam is built now because everything
downstream either sits on it or splits along it.

**Phases 4 and 5 are breadth on a stable spine.** Views generalize one scene into many; materials
and submission add the data-driven content path and the real sort-and-batch scaling work. Both
lean on seams the spine already established, so they are largely additive.

**Phase 6 is deferred until measured.** The frame-graph compiler, transient pooling, and the
render thread are held back deliberately: the graph machinery is over-engineering until pass
count earns compiling, culling, and pooling, and the render thread is worth its complexity only
when profiling says the main thread is the limit. By the time this phase begins, every seam it
needs already exists.

**Start point.** Phase 0 is closed — its decisions are recorded above. Begin at Phase 1: the RHI
crate and the frame ring, since everything above holds handles and every later phase assumes the
frame plumbing is already in place.
