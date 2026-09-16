struct Globals {
    screen: vec2<f32>,
    _pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var tex_sampler: sampler;

struct Instance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) kind: u32,
    @location(5) color2: u32,
    @location(6) phase: f32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) kind: u32,
    @location(3) local: vec2<f32>,
    @location(4) @interpolate(flat) size: vec2<f32>,
    @location(5) @interpolate(flat) params: vec2<f32>,
    @location(6) @interpolate(flat) color2: vec4<f32>,
    @location(7) @interpolate(flat) phase: f32,
};

fn unpack(c: u32) -> vec4<f32> {
    return vec4(f32(c & 0xffu), f32((c >> 8u) & 0xffu), f32((c >> 16u) & 0xffu), f32((c >> 24u) & 0xffu)) / 255.0;
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let c = corners[vi];
    let px = inst.pos + c * inst.size;
    let ndc = vec2(px.x / globals.screen.x * 2.0 - 1.0, 1.0 - px.y / globals.screen.y * 2.0);
    var out: VsOut;
    out.clip = vec4(ndc, 0.0, 1.0);
    out.uv = mix(inst.uv.xy, inst.uv.zw, c);
    out.color = inst.color;
    out.kind = inst.kind;
    out.local = c * inst.size;
    out.size = inst.size;
    out.params = inst.uv.xy;
    out.color2 = unpack(inst.color2);
    out.phase = inst.phase;
    return out;
}

// Signed distance to a rounded box of half-size `b` and radius `r`, centered at 0.
fn sd_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2(r, r);
    return length(max(q, vec2(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// kind 0: solid. 1: atlas glyph (R = coverage). 2: external RGBA texture.
// 3: rounded fill (params.x = radius). 4: rounded stroke (params.x = radius,
// params.y = thickness). 3 and 4 blend toward color2 along a diagonal
// gradient when color2.a > 0; `phase` slides it (aurora).
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if in.kind == 0u {
        return in.color;
    }
    if in.kind == 1u {
        let s = textureSample(tex, tex_sampler, in.uv);
        return vec4(in.color.rgb, in.color.a * s.r);
    }
    if in.kind == 2u {
        let s = textureSample(tex, tex_sampler, in.uv);
        return vec4(s.rgb, s.a * in.color.a);
    }
    let half = in.size * 0.5;
    let p = in.local - half;
    let d = sd_box(p, half, in.params.x);
    var cov = 1.0 - smoothstep(-0.75, 0.75, d);
    if in.kind == 4u {
        let inner = sd_box(p, half - vec2(in.params.y, in.params.y), max(in.params.x - in.params.y, 0.0));
        cov = cov * smoothstep(-0.75, 0.75, inner);
    }
    var color = in.color;
    if in.color2.a > 0.0 {
        let t = (in.local.x + in.local.y) / (in.size.x + in.size.y);
        let g = 0.5 + 0.5 * sin(6.2831853 * (t + in.phase));
        color = mix(in.color, in.color2, g);
    }
    return vec4(color.rgb, color.a * cov);
}
