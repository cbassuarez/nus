struct Globals {
    screen: vec2<f32>,
    corner_radius: f32,
    padding: f32,
};
var<immediate> globals: Globals;
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var tex_sampler: sampler;
// Polygon corners for kind 13, relative to the instance's box.
@group(1) @binding(0) var<storage, read> points: array<vec2<f32>>;

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
    @location(11) @interpolate(flat) raw2: u32,
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
    out.raw2 = inst.color2;
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

// The sky (kind 14): value noise, fBm, and a day that turns with the sun.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    var f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}
fn fbm(p0: vec2<f32>) -> f32 {
    var p = p0;
    var s = 0.0;
    var a = 0.5;
    let m = mat2x2<f32>(vec2(1.6, 1.2), vec2(-1.2, 1.6));
    for (var i = 0; i < 6; i = i + 1) {
        s = s + a * vnoise(p);
        p = m * p + vec2(3.1, 1.7);
        a = a * 0.5;
    }
    return s;
}
fn sky_at(y: f32, alt: f32) -> vec3<f32> {
    let day = smoothstep(-0.12, 0.25, alt);
    let dusk = smoothstep(-0.25, 0.0, alt) * (1.0 - smoothstep(0.0, 0.3, alt));
    let zen = mix(vec3(0.02, 0.03, 0.07), vec3(0.16, 0.38, 0.82), day);
    let hor = mix(vec3(0.05, 0.06, 0.11), vec3(0.70, 0.82, 0.94), day);
    var c = mix(hor, zen, pow(clamp(y, 0.0, 1.0), 0.55));
    c = mix(c, vec3(1.0, 0.55, 0.28), dusk * pow(1.0 - clamp(y, 0.0, 1.0), 2.5) * 0.85);
    return c;
}
// A bounded single-scattering approximation: wavelength-dependent Rayleigh
// extinction, Rayleigh phase and forward Mie scattering. Twilight remains an
// artistic approximation, not a claim to Bruneton multiple-scattering accuracy.
fn atmospheric_day(view: vec3<f32>, sun: vec3<f32>, sun_height: f32) -> vec3<f32> {
    let mu = clamp(dot(view, sun), -1.0, 1.0);
    let rayleigh = vec3(0.0464, 0.108, 0.2648); // sea-level beta * 8 km scale height
    let haze = vec3(0.025);
    let view_mass = 1.0 / max(0.045, view.y + 0.025);
    let sun_mass = 1.0 / max(0.025, sun_height + 0.025);
    let sunlight = exp(-(rayleigh + haze) * sun_mass);
    let extinction = exp(-(rayleigh + haze) * view_mass);
    let ray_phase = 0.75 * (1.0 + mu * mu);
    let g = 0.76;
    let mie_phase = (1.0 - g*g) / pow(max(0.02, 1.0 + g*g - 2.0*g*mu), 1.5);
    let scatter = (rayleigh * ray_phase + haze * mie_phase) / (rayleigh + haze);
    let direct = (vec3(1.0) - extinction) * scatter * sunlight;
    let ambient = (vec3(1.0) - exp(-rayleigh * view_mass)) * vec3(0.19, 0.28, 0.42);
    return pow(vec3(1.0) - exp(-(direct * 1.6 + ambient)), vec3(1.0/2.2));
}

fn sky(in: VsOut) -> vec4<f32> {
    let uv = vec2(in.local.x / in.size.x, 1.0 - in.local.y / in.size.y);
    let ar = in.size.x / in.size.y;
    let az = in.params.x;
    let alt = in.params.y;
    let cover = in.color.r;
    let wind = in.color.g;
    let pan = vec2(in.color.b, in.color.a);
    let t = in.phase;
    // The horizon a little under the pane: a sky looked up at.
    let y = uv.y * 0.9 + 0.08;
    let sp = vec2(0.5 + az * 0.55, alt * 0.9 + 0.02);
    let view = normalize(vec3((uv.x - 0.5) * ar * 1.6, y * 1.6, 1.0));
    let sun_dir = normalize(vec3((sp.x - 0.5) * ar * 1.6, max(0.0,alt) * 1.6, 1.0));
    var col = mix(sky_at(y, alt), atmospheric_day(view,sun_dir,alt), smoothstep(-0.015,0.12,alt));
    let day = smoothstep(-0.12, 0.25, alt);
    // The sun: a disc and a glow where it is.
    let dist = length((uv - sp) * vec2(ar, 1.0));
    let sun_col = mix(vec3(1.0, 0.75, 0.45), vec3(1.0, 0.98, 0.92), smoothstep(0.0, 0.35, alt));
    col = col + sun_col * (0.9 * exp(-dist * 28.0) + 0.35 * exp(-dist * 6.0)) * step(-0.12, alt);
    col = col + sun_col * smoothstep(0.018, 0.012, dist) * step(-0.05, alt);
    // A dated lunar position and phase, visible by day too; no fictional
    // full Moon opposite every Sun. 16-bit positions avoid quantized drift.
    let moon = unpack2x16unorm(in.raw2) * 2.0 - vec2(1.0);
    let moon_light = f32(in.extra & 65535u) / 65535.0;
    let mp = vec2(0.5 + moon.x * 0.55, moon.y * 0.9 + 0.02);
    let moon_uv = (uv - mp) * vec2(ar,1.0) / 0.010;
    let md = length(moon_uv);
    let z = sqrt(max(0.0,1.0-dot(moon_uv,moon_uv)));
    let toward_sun = normalize((sp-mp)*vec2(ar,1.0)+vec2(0.00001));
    let phase_z = 2.0*moon_light-1.0;
    let normal_light = dot(moon_uv,toward_sun)*sqrt(max(0.0,1.0-phase_z*phase_z))+z*phase_z;
    let lunar = smoothstep(-0.025,0.035,normal_light) * (1.0-smoothstep(0.94,1.03,md));
    let lunar_visible = smoothstep(-0.015,0.01,moon.y);
    col = mix(col,vec3(0.87,0.89,0.92),lunar*lunar_visible*(0.9-0.45*day));
    col += vec3(0.08,0.09,0.12)*exp(-md*0.2)*moon_light*lunar_visible*(1.0-day);
    let cell = floor(uv * in.size * 0.5);
    // Atmospheric scintillation strengthens toward the horizon; positions stay fixed.
    let twinkle = 0.88 + (0.025 + 0.08*(1.0-y))*sin(t*2.3+hash(cell)*40.0);
    let st = step(0.997, hash(cell)) * (1.0-day) * twinkle * smoothstep(0.0,0.2,y);
    col = col + vec3(st);
    // Clouds: flatter and denser toward the horizon; the field warps itself.
    let z = 1.0 / (y * 1.25 + 0.22);
    let p = vec2((uv.x - 0.5) * ar * z * 0.95, z * 0.9) * 1.45 + pan;
    let w = vec2(wind * t * 0.010, wind * t * 0.0015);
    let q = p + w;
    let warp = 0.35 * vec2(fbm(q * 0.9 + vec2(1.7, 9.2)), fbm(q * 0.9 + vec2(8.3, 2.8)));
    let base = fbm(q + warp) * 0.8 + fbm((q + warp * 0.5) * 3.1 + 7.0) * 0.3;
    let edge = 0.62 - cover * 0.42;
    var dens = smoothstep(edge - 0.06, edge + 0.26, base);
    let core = smoothstep(edge + 0.12, edge + 0.5, base);
    dens = dens * smoothstep(0.0, 0.2, y);
    // Lit from the sun: the field a step toward it, for a cheap normal.
    let to_sun = normalize(vec2(az * 0.6, max(alt, 0.08)));
    let q2 = q + to_sun * 0.05;
    let warp2 = 0.35 * vec2(fbm(q2 * 0.9 + vec2(1.7, 9.2)), fbm(q2 * 0.9 + vec2(8.3, 2.8)));
    let nl = fbm(q2 + warp2) * 0.8 + fbm((q2 + warp2 * 0.5) * 3.1 + 7.0) * 0.3;
    let lit = clamp(0.5 + (nl - base) * 9.0, 0.0, 1.0);
    var shade = mix(vec3(0.72, 0.76, 0.86), vec3(0.30, 0.32, 0.40), 1.0 - day);
    var light = mix(vec3(1.0), sun_col, 0.25);
    light = mix(vec3(0.22, 0.24, 0.32), light, day);
    let dusk_t = smoothstep(-0.2, 0.0, alt) * (1.0 - smoothstep(0.0, 0.3, alt));
    light = mix(light, vec3(1.0, 0.66, 0.45), dusk_t * 0.7);
    shade = mix(shade, vec3(0.55, 0.42, 0.48), dusk_t * 0.5);
    var cc = mix(shade, light, lit);
    cc = mix(cc, light, core * 0.35);
    let rim = smoothstep(0.0, 0.1, dens) * (1.0 - smoothstep(0.1, 0.4, dens));
    cc = cc + light * rim * 0.3 * day;
    col = mix(col, cc, min(1.0, dens * 1.05));
    // A hair of grain so the gradient never bands.
    col = col + (hash(uv * in.size) - 0.5) * 0.012;
    return vec4(col, 1.0);
}

// kind 0: solid. 1: atlas glyph (R = coverage). 2: external RGBA texture.
// 3: rounded fill (params.x = radius). 4: rounded stroke (params.x = radius,
// params.y = thickness). 3 and 4 blend toward color2 along a diagonal
// gradient when color2.a > 0; `phase` slides it (aurora).
fn shade(in: VsOut) -> vec4<f32> {
    if in.kind == 0u {
        return in.color;
    }
    if in.kind == 14u {
        return sky(in);
    }
    if in.kind == 13u {
        // A polygon: `extra` is where its corners start in `points`, raw2
        // how many. Signed distance to the outline (even-odd inside) gives
        // one anti-aliased edge and no seams within, however translucent.
        let start = in.extra;
        let n = in.raw2;
        let p = in.local;
        var d2 = 1.0e18;
        var inside = false;
        for (var i = 0u; i < n; i = i + 1u) {
            let a = points[start + i];
            let b = points[start + (i + 1u) % n];
            let e = b - a;
            let w = p - a;
            let t = clamp(dot(w, e) / max(dot(e, e), 1.0e-6), 0.0, 1.0);
            let q = w - e * t;
            d2 = min(d2, dot(q, q));
            if (a.y <= p.y) != (b.y <= p.y) {
                let x = a.x + (p.y - a.y) * e.x / e.y;
                if p.x < x {
                    inside = !inside;
                }
            }
        }
        var sd = sqrt(d2);
        if inside {
            sd = -sd;
        }
        let cov = 1.0 - smoothstep(-0.75, 0.75, sd);
        return vec4(in.color.rgb, in.color.a * cov);
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

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let color = shade(in);
    let radius = min(globals.corner_radius, min(globals.screen.x, globals.screen.y) * 0.5);
    if radius <= 0.0 { return color; }
    let half_size = globals.screen * 0.5;
    let q = abs(in.clip.xy - half_size) - half_size + vec2(radius);
    let distance = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    return color * (1.0 - smoothstep(-0.5, 0.5, distance));
}
