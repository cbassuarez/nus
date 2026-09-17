struct Globals {
    screen: vec2<f32>,
};
var<immediate> globals: Globals;
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var tex_sampler: sampler;

struct Instance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) kind: u32,
    @location(5) color2: u32,
    @location(6) phase: f32,
    @location(7) extra: u32,
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
    @location(8) @interpolate(flat) extra: u32,
    @location(9) @interpolate(flat) stop3: vec4<f32>,
    @location(10) @interpolate(flat) stop4: vec4<f32>,
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
    var px = inst.pos + c * inst.size;
    // Glyph instances may spin about their centre: phase is the angle.
    if inst.kind == 1u && inst.phase != 0.0 {
        let half = inst.size * 0.5;
        let d = c * inst.size - half;
        let cs = cos(inst.phase);
        let sn = sin(inst.phase);
        px = inst.pos + half + vec2(d.x * cs - d.y * sn, d.x * sn + d.y * cs);
    }
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
    out.extra = inst.extra;
    out.stop3 = unpack(bitcast<u32>(inst.uv.z));
    out.stop4 = unpack(bitcast<u32>(inst.uv.w));
    return out;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

// A colour ramp through up to four stops; when `looping` the ramp returns
// to the first stop so a drifting `t` never jumps.
fn ramp(t: f32, n: u32, looping: bool, c0: vec4<f32>, c1: vec4<f32>, c2: vec4<f32>, c3: vec4<f32>) -> vec4<f32> {
    var segs = f32(n - 1u);
    if looping {
        segs = f32(n);
    }
    let x = t * segs;
    let i = u32(floor(x));
    let f = smoothstep(0.0, 1.0, fract(x));
    var a = c0;
    var b = c1;
    let i0 = i % n;
    let i1 = (i + 1u) % n;
    if i0 == 1u { a = c1; } else if i0 == 2u { a = c2; } else if i0 == 3u { a = c3; }
    if i1 == 0u { b = c0; } else if i1 == 2u { b = c2; } else if i1 == 3u { b = c3; }
    return mix(a, b, f);
}

// Finish a texture sample: light or dark speckle so it reads on any colour,
// masked to the instance's rounded stroke when it has one.
fn texture_out(in: VsOut, v: f32, light: bool) -> vec4<f32> {
    var mask = 1.0;
    if in.params.y > 0.0 {
        let half = in.size * 0.5;
        let p = in.local - half;
        let d = sd_box(p, half, in.params.x);
        let inner = sd_box(p, half - vec2(in.params.y, in.params.y), max(in.params.x - in.params.y, 0.0));
        mask = (1.0 - smoothstep(-0.75, 0.75, d)) * smoothstep(-0.75, 0.75, inner);
    }
    var tone = vec3(0.0, 0.0, 0.0);
    if light {
        tone = vec3(1.0, 1.0, 1.0);
    }
    return vec4(tone, in.color.a * v * mask);
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
    if in.kind == 12u {
        // A convex quad: four corners inside the instance box, c0 in params,
        // c1 and c2 as two u16 each in uv.z / uv.w (seen here as bytes), c3
        // in color2's bytes. Signed distance to the polygon gives the edge.
        let c0 = in.params * in.size;
        let c1 = vec2(in.stop3.r * 255.0 + in.stop3.g * 255.0 * 256.0, in.stop3.b * 255.0 + in.stop3.a * 255.0 * 256.0) / 65535.0 * in.size;
        let c2 = vec2(in.stop4.r * 255.0 + in.stop4.g * 255.0 * 256.0, in.stop4.b * 255.0 + in.stop4.a * 255.0 * 256.0) / 65535.0 * in.size;
        let c3 = vec2(in.color2.r * 255.0 + in.color2.g * 255.0 * 256.0, in.color2.b * 255.0 + in.color2.a * 255.0 * 256.0) / 65535.0 * in.size;
        let centre = (c0 + c1 + c2 + c3) * 0.25;
        var corners = array<vec2<f32>, 4>(c0, c1, c2, c3);
        var sd = -1.0e9;
        for (var i = 0u; i < 4u; i = i + 1u) {
            let a = corners[i];
            let b = corners[(i + 1u) % 4u];
            let e = b - a;
            let len = max(length(e), 1.0e-4);
            var n = vec2(e.y, -e.x) / len;
            if dot(n, centre - a) > 0.0 {
                n = -n;
            }
            sd = max(sd, dot(n, in.local - a));
        }
        let cov = 1.0 - smoothstep(-0.75, 0.75, sd);
        return vec4(in.color.rgb, in.color.a * cov);
    }
    // Textures: `extra` is time in ms (0 = still). Grain reseeds like film;
    // the patterns drift slowly. Each texture yields a value in 0..1 and a
    // light/dark tone; `texture_out` masks it to a rounded stroke when the
    // instance carries one (params = radius, thickness), so a carapace
    // texture follows the carapace.
    let tm = f32(in.extra) / 1000.0;
    let drift = vec2(tm * 6.0, tm * 2.5);
    if in.kind == 6u {
        // Paper grain: hashed speckle in screen space, alpha scaled by color.a.
        let p = floor(in.clip.xy / max(in.phase, 1.0)) + floor(tm * 24.0) * vec2(17.0, 31.0);
        let n = fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
        return texture_out(in, abs(n - 0.5) * 2.0, n > 0.5);
    }
    if in.kind == 7u {
        // Stipple: dots on a jittered grid, `phase` px apart.
        let pitch = max(in.phase, 2.0);
        let cell = floor((in.clip.xy + drift) / pitch);
        let j = vec2(hash(cell), hash(cell + vec2(7.0, 3.0))) * 0.5 - 0.25;
        let c = (cell + 0.5 + j) * pitch;
        let d = length(in.clip.xy + drift - c);
        let r = pitch * 0.18;
        let cov = 1.0 - smoothstep(r - 0.6, r + 0.6, d);
        return texture_out(in, cov, hash(cell + vec2(3.0, 9.0)) > 0.5);
    }
    if in.kind == 8u {
        // Stitch: a dashed cross-hatch, like thread — lines every `phase` px.
        let pitch = max(in.phase, 3.0);
        let q = (in.clip.xy + drift) / pitch;
        let lx = abs(fract(q.x) - 0.5);
        let ly = abs(fract(q.y) - 0.5);
        let dash_x = step(0.5, fract(q.y * 2.0 + 0.25));
        let dash_y = step(0.5, fract(q.x * 2.0));
        let line = max((1.0 - smoothstep(0.04, 0.09, lx)) * dash_x, (1.0 - smoothstep(0.04, 0.09, ly)) * dash_y);
        return texture_out(in, line, fract(floor(q.x) * 0.5) < 0.25);
    }
    if in.kind == 9u {
        // Linen: two fine directions of slightly uneven threads.
        let pitch = max(in.phase, 1.5);
        let q = (in.clip.xy + drift * 0.3) / pitch;
        let wx = 0.5 + 0.5 * sin(6.2831853 * q.x) * (0.8 + 0.2 * hash(floor(q.yx)));
        let wy = 0.5 + 0.5 * sin(6.2831853 * q.y) * (0.8 + 0.2 * hash(floor(q.xy)));
        let w = max(wx, wy) * 0.6 + 0.4 * wx * wy;
        return texture_out(in, w, wx > wy);
    }
    if in.kind == 10u {
        // Halftone: a dot screen at 30 degrees whose dots swell with a slow field.
        let pitch = max(in.phase, 3.0);
        let a = 0.5235988;
        let rp = vec2(in.clip.x * cos(a) - in.clip.y * sin(a), in.clip.x * sin(a) + in.clip.y * cos(a));
        let cell = floor(rp / pitch);
        let c = (cell + 0.5) * pitch;
        let d = length(rp - c);
        let field = 0.5 + 0.5 * sin(in.clip.x * 0.01 + tm * 0.7) * sin(in.clip.y * 0.013 - tm * 0.5);
        let r = pitch * (0.12 + 0.28 * field);
        let cov = 1.0 - smoothstep(r - 0.6, r + 0.6, d);
        return texture_out(in, cov, true);
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
    if in.kind == 4u || in.kind == 5u {
        let inner = sd_box(p, half - vec2(in.params.y, in.params.y), max(in.params.x - in.params.y, 0.0));
        cov = cov * smoothstep(-0.75, 0.75, inner);
    }
    var color = in.color;
    if in.kind == 5u {
        // Hazard tape: diagonal stripes of color / color2, `phase` px per stripe.
        let s = (in.local.x + in.local.y) / max(in.phase, 1.0);
        if fract(s * 0.5) >= 0.5 {
            color = in.color2;
        }
        return vec4(color.rgb, color.a * cov);
    }
    let n = in.extra & 0xffu;
    if n >= 2u {
        // Multi-stop ramp along `angle`; looping wraps for the aurora drift.
        let ang = f32((in.extra >> 8u) & 0x3ffu) * 0.017453292;
        let dir = vec2(cos(ang), sin(ang));
        let ext = abs(dir.x) * in.size.x + abs(dir.y) * in.size.y;
        var t = (dot(in.local - in.size * 0.5, dir) / max(ext, 1.0)) + 0.5;
        let looping = ((in.extra >> 18u) & 1u) == 1u;
        if looping {
            t = fract(t + in.phase);
        } else {
            t = clamp(t, 0.0, 1.0);
        }
        color = ramp(t, n, looping, in.color, in.color2, in.stop3, in.stop4);
    } else if in.color2.a > 0.0 {
        let t = (in.local.x + in.local.y) / (in.size.x + in.size.y);
        let g = 0.5 + 0.5 * sin(6.2831853 * (t + in.phase));
        color = mix(in.color, in.color2, g);
    }
    return vec4(color.rgb, color.a * cov);
}
