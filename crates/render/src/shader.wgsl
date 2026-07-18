// Instanced colored quads. Each instance is a rectangle in *pixel* space
// (center + half-extent); the vertex shader maps pixels to normalized device
// coordinates using the target size in `globals`. Instances flagged as circles
// are drawn as smooth-edged discs in the fragment shader.

struct Globals {
    screen_size: vec2<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;

struct VsIn {
    @location(0) corner: vec2<f32>,      // unit-quad corner in [0,1]^2
    @location(1) center: vec2<f32>,      // instance center, pixels
    @location(2) half_size: vec2<f32>,   // instance half-extent, pixels
    @location(3) color: vec4<f32>,
    @location(4) circle: f32,            // 1 = disc, 0 = rectangle
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local: vec2<f32>,       // [-1,1]^2 across the quad
    @location(2) circle: f32,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let local = in.corner * 2.0 - 1.0;               // 0..1 -> -1..1
    let pixel = in.center + local * in.half_size;
    // Pixel space (y down from top) -> NDC (y up).
    let ndc = vec2<f32>(
        pixel.x / globals.screen_size.x * 2.0 - 1.0,
        1.0 - pixel.y / globals.screen_size.y * 2.0,
    );
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.color = in.color;
    out.local = local;
    out.circle = in.circle;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if in.circle > 0.5 {
        // Distance from center; fade the last pixel for a smooth disc edge.
        let d = length(in.local);
        let edge = fwidth(d);
        let alpha = 1.0 - smoothstep(1.0 - edge, 1.0, d);
        if alpha <= 0.0 {
            discard;
        }
        return vec4<f32>(in.color.rgb, in.color.a * alpha);
    }
    return in.color;
}
