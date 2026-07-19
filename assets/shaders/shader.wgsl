struct ObjectUniform {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    tint: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> u: ObjectUniform;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    let world_pos = u.model * vec4<f32>(v.pos, 1.0);
    o.pos = u.view_proj * world_pos;
    o.world_normal = (u.model * vec4<f32>(v.normal, 0.0)).xyz;
    return o;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    let n_len2 = dot(i.world_normal, i.world_normal);
    if (n_len2 < 1e-6) {
        return vec4<f32>(u.tint.rgb, 1.0);
    }

    let n = normalize(i.world_normal);
    let light_dir = normalize(vec3<f32>(-0.6, -1.0, -0.3));
    let ambient = 0.25;
    let diff = max(dot(n, -light_dir), 0.0);
    return vec4<f32>(u.tint.rgb * (ambient + diff), 1.0);
}
