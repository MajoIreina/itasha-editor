@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var history: texture_2d<f32>;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var history_depth: texture_2d<f32>;
@group(0) @binding(4) var tex_sampler: sampler;
struct Params {
    inverse_jittered: mat4x4<f32>,
    current: mat4x4<f32>,
    previous: mat4x4<f32>,
    options: vec4<f32>,
}
@group(0) @binding(5) var<uniform> params: Params;
struct Out { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) index: u32) -> Out {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return Out(vec4(uv * 2. - 1., 0., 1.), vec2(uv.x, 1. - uv.y));
}
struct Resolved { @location(0) color: vec4<f32>, @location(1) depth: f32 }
fn linear_depth(z: f32) -> f32 { return 0.03 * 200. / max(200. - z * (200. - 0.03), 0.03); }
fn ycocg(c: vec3<f32>) -> vec3<f32> { return vec3(dot(c, vec3(0.25, 0.5, 0.25)), (c.r - c.b) * 0.5, dot(c, vec3(-0.25, 0.5, -0.25))); }
fn rgb(c: vec3<f32>) -> vec3<f32> { return vec3(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z); }
@fragment fn fs(o: Out) -> Resolved {
    let size = vec2<i32>(textureDimensions(scene));
    let pixel = clamp(vec2<i32>(o.p.xy), vec2(0), size - vec2(1));
    let current = textureLoad(scene, pixel, 0).rgb;
    let z = textureLoad(depth, pixel, 0);
    var result = Resolved(vec4(current, 1.), linear_depth(z));
    if params.options.y < 0.5 { return result; }
    // The ground and glass do not write their own depth. Reject background
    // history during camera motion and cap moving history weight globally.
    if z >= 0.99999 && params.options.z > 0.5 { return result; }
    let world_h = params.inverse_jittered * vec4(o.uv * vec2(2., -2.) + vec2(-1., 1.), z, 1.);
    let world = vec4(world_h.xyz / world_h.w, 1.);
    let now = params.current * world;
    let old = params.previous * world;
    if old.w <= 0. { return result; }
    // Motion excludes projection jitter: a stationary pixel always looks up
    // the same history pixel, preserving its distinct subpixel samples.
    let history_uv = o.uv + (old.xy / old.w - now.xy / now.w) * vec2(0.5, -0.5);
    if any(history_uv < vec2(0.)) || any(history_uv > vec2(1.)) { return result; }
    let hp = vec2<i32>(floor(history_uv * vec2<f32>(size) - vec2(0.5)));
    var depth_error = 1e10;
    for (var y = 0; y <= 1; y++) {
        for (var x = 0; x <= 1; x++) {
            let previous_depth = textureLoad(history_depth, clamp(hp + vec2(x,y), vec2(0), size-vec2(1)), 0).r;
            depth_error = min(depth_error, abs(previous_depth - old.w));
        }
    }
    if depth_error > max(0.02, old.w * 0.015) { return result; }
    var low = ycocg(current);
    var high = low;
    var mean = vec3(0.);
    var square = vec3(0.);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let c = ycocg(textureLoad(scene, clamp(pixel + vec2(x,y), vec2(0), size-vec2(1)), 0).rgb);
            low = min(low, c); high = max(high, c);
            mean += c; square += c * c;
        }
    }
    mean /= 9.;
    let sigma = sqrt(max(square / 9. - mean * mean, vec3(0.)));
    low = max(low, mean - sigma * 1.5); high = min(high, mean + sigma * 1.5);
    let old_color = ycocg(textureSampleLevel(history, tex_sampler, history_uv, 0.).rgb);
    let clipped = rgb(clamp(old_color, low, high));
    result.color = vec4(mix(current, max(clipped, vec3(0.)), params.options.x), 1.);
    return result;
}
