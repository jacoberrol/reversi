// Instanced quads. Each instance is a rectangle in *pixel* space (center +
// half-extent); the vertex shader maps pixels to normalized device coordinates
// using the target size in `globals`. `shape`/`param` select how the fragment
// shader paints it:
//   shape 0 = plain rect
//   shape 1 = rounded rect  (param = corner radius, fraction of half-extent)
//   shape 2 = disc          (param = gloss: 0 matte, 1 glossy)

struct Globals {
    screen_size: vec2<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;

struct VsIn {
    @location(0) corner: vec2<f32>,      // unit-quad corner in [0,1]^2
    @location(1) center: vec2<f32>,      // instance center, pixels
    @location(2) half_size: vec2<f32>,   // instance half-extent, pixels
    @location(3) color: vec4<f32>,
    @location(4) shape: f32,
    @location(5) param: f32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local: vec2<f32>,                    // [-1,1]^2 across the quad
    @location(2) @interpolate(flat) shape: f32,
    @location(3) @interpolate(flat) param: f32,
    @location(4) half_size: vec2<f32>,
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
    out.shape = in.shape;
    out.param = in.param;
    out.half_size = in.half_size;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if in.shape < 0.5 {
        // Plain rectangle.
        return in.color;
    } else if in.shape < 1.5 {
        // Rounded rectangle: signed distance to a rounded box in local units,
        // scaled to pixels for a crisp 1px antialiased edge.
        let r = clamp(in.param, 0.0, 1.0);
        let q = abs(in.local) - (vec2<f32>(1.0, 1.0) - vec2<f32>(r, r));
        let dist = length(max(q, vec2<f32>(0.0, 0.0))) - r;
        let px = dist * min(in.half_size.x, in.half_size.y);
        let aa = fwidth(px);
        let alpha = 1.0 - smoothstep(0.0, aa, px);
        if alpha <= 0.0 {
            discard;
        }
        return vec4<f32>(in.color.rgb, in.color.a * alpha);
    } else {
        // Disc. `local` distance 1.0 is the rim.
        let d = length(in.local);
        let aa = fwidth(d);
        let alpha = 1.0 - smoothstep(1.0 - aa, 1.0, d);
        if alpha <= 0.0 {
            discard;
        }

        var rgb = in.color.rgb;
        if in.param > 0.5 {
            // Rim shadow: darken toward the edge for a rounded, 3D read.
            let rim = smoothstep(0.72, 1.0, d);
            rgb = mix(rgb, rgb * 0.5, rim * 0.85);
            // Specular highlight: a soft bright spot offset toward upper-left.
            let hd = distance(in.local, vec2<f32>(-0.35, -0.4));
            let hi = (1.0 - smoothstep(0.0, 0.75, hd)) * 0.5;
            rgb = rgb + vec3<f32>(hi, hi, hi);
        }
        return vec4<f32>(rgb, in.color.a * alpha);
    }
}
