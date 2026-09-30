// Ground sky: a slowly refreshed cloud volume, then a cheap world-space reprojection.
// All directions use East, Up, North. Weather constrains modeled cloud shapes.
struct Params {
    resolution: vec4<f32>, // output width, height, aspect, volume overscan
    sun: vec4<f32>,        // EUN direction, lunar illumination
    moon: vec4<f32>,       // EUN direction, waxing sign
    right: vec4<f32>,      // camera right, tan(vertical FOV / 2)
    up: vec4<f32>,         // camera up
    forward: vec4<f32>,    // camera forward
    weather: vec4<f32>,    // low, middle, high cover, stratus fraction
    air: vec4<f32>,        // precipitation mm/h, haze, low base km, animation seconds
    wind_low: vec4<f32>,   // east and north metres/second
    wind_mid: vec4<f32>,
    wind_high: vec4<f32>,
    cache: vec4<f32>,      // reference time, reprojection plane km, noise offset, unused
    cache_wind: vec4<f32>, // low cloud displacement at cache generation, km
    reading: vec4<f32>,    // top-left normalized x, y, width, height
    protection: vec4<f32>, // reading strength
}
@group(0) @binding(0) var<uniform> u: Params;
@group(0) @binding(1) var noise_texture: texture_3d<f32>;
@group(0) @binding(2) var noise_sampler: sampler;
@group(0) @binding(3) var cloud_texture: texture_2d<f32>;
@group(0) @binding(4) var cloud_sampler: sampler;
struct VertexOut { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> VertexOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: VertexOut;
    out.position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
    // Clip-space +Y points upward; texture-coordinate conversion is in presentation.
    out.uv = p;
    return out;
}
fn sat(v: f32) -> f32 { return clamp(v, 0.0, 1.0); }
fn noise(p: vec3<f32>) -> vec4<f32> { return textureSampleLevel(noise_texture, noise_sampler, p, 0.0); }
fn n3(p: vec3<f32>) -> f32 { return noise(p).r; }
fn rain() -> f32 { return 1.0 - exp(-u.air.x * 0.45); }
fn cloud_base() -> f32 { return max(0.15, u.air.z); }
fn cloud_top() -> f32 { return cloud_base() + mix(3.60, 1.95, u.weather.w); }
fn ray(uv: vec2<f32>, overscan: f32) -> vec3<f32> {
    let xy = (uv * 2.0 - 1.0) * u.right.w * overscan;
    return normalize(u.forward.xyz + u.right.xyz * xy.x * u.resolution.z + u.up.xyz * xy.y);
}
fn density(point: vec3<f32>) -> f32 {
    let o = u.weather.w; let r = rain();
    var p = point;
    // Positive wind moves clouds toward that world-space direction.
    p.x -= u.wind_low.z;
    p.z -= u.wind_low.w;
    var base = cloud_base();
    let top = cloud_top();
    if o > 0.001 {
        let lp = p.xz * 0.13;
        let roll = noise(vec3(lp, 0.36)).g;
        let small = n3(vec3(lp * 2.31, 0.73));
        base += o * (0.12 + 0.65 * roll + 0.19 * small);
    }
    let h = (p.y - base) / max(0.15, top - base);
    if h <= 0.0 || h >= 1.0 { return 0.0; }
    let wp = p.xz * 0.023 + vec2(0.177, 0.063);
    let w = n3(vec3(wp, 0.41)); let w2 = n3(vec3(wp * 2.73, 0.73));
    var coverage = u.weather.x + (w - 0.5) * 0.84 + (w2 - 0.5) * 0.21;
    // One connected frontal bank rather than independent dark storm blobs.
    let front = smoothstep(-1.0, 8.0, p.x + sin(p.z * 0.10) * 2.0);
    coverage = mix(coverage, mix(max(0.18, u.weather.x * 0.50), 1.02, front), r);
    let np = p * vec3(0.071, 0.103, 0.071) + vec3(0.174, 0.037, 0.583);
    let a = noise(np); let shape = a.r * 0.60 + a.g * 0.40;
    var cloud_shape = shape - (1.0 - coverage) * 0.82;
    let vertical = smoothstep(0.0, 0.10, h) * (1.0 - smoothstep(0.30, 1.0, h));
    cloud_shape -= (1.0 - vertical) * 0.32;
    let b = noise(np * 3.37 + vec3(0.27, 0.71, 0.14));
    let fine = noise(np * 11.71 + vec3(0.51, 0.19, 0.63));
    var erosion = (1.0 - b.g) * 0.19 + (1.0 - b.b) * 0.095 + (1.0 - fine.b) * 0.024;
    erosion += (1.0 - noise(np * 29.31 + 0.12).a) * 0.010;
    let body = cloud_shape - erosion + 0.084 + o * 0.025;
    return sat(body * mix(18.0, 3.1, o)) * mix(0.94, 1.50, r) * smoothstep(0.0, 0.07, h);
}
fn phase_hg(mu: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / pow(max(0.04, 1.0 + g2 - 2.0 * g * mu), 1.5);
}
fn sky_color(d: vec3<f32>) -> vec3<f32> {
    let sy = u.sun.y; let day = smoothstep(-0.15, 0.12, sy);
    let dusk = exp(-pow((sy - 0.035) * 5.2, 2.0));
    let horizon_amount = pow(1.0 - sat(d.y), 3.0); let mu = dot(d, u.sun.xyz);
    var zenith = mix(vec3(0.0015, 0.0035, 0.011), vec3(0.025, 0.155, 0.395), day);
    var horizon = mix(vec3(0.009, 0.014, 0.026), vec3(0.52, 0.69, 0.83), day);
    let toward = pow(sat(mu * 0.5 + 0.5), 5.0);
    horizon = mix(horizon, vec3(0.98, 0.40, 0.13), dusk * (0.26 + 0.60 * toward));
    zenith = mix(zenith, vec3(0.10, 0.135, 0.275), dusk * 0.24);
    var sky = mix(zenith, horizon, horizon_amount);
    sky += vec3(1.0, 0.67, 0.37) * pow(sat(mu), 16.0) * 0.15 * day;
    sky += vec3(1.0, 0.90, 0.70) * pow(sat(mu), 80.0) * 0.11 * day;
    sky = mix(sky, vec3(0.44, 0.51, 0.59) * (0.24 + 0.70 * day), u.weather.w * 0.26);
    let haze = (1.0 - exp(-max(0.0, u.air.y) / max(0.10, d.y))) * 0.45;
    sky = mix(sky, horizon, haze);
    // Moonlight is a faint directional halo, never an invented opposite Sun.
    sky += vec3(0.018, 0.025, 0.042) * pow(sat(dot(d, u.moon.xyz)), 120.0)
        * u.sun.w * smoothstep(-0.02, 0.10, u.moon.y) * (1.0 - day);
    return sky;
}
fn light_direction() -> vec3<f32> {
    return select(u.moon.xyz, u.sun.xyz, u.sun.y > -0.055);
}
fn shadow(point: vec3<f32>, direction: vec3<f32>) -> f32 {
    var p = point; var optical = 0.0; var step_size = 0.12;
    for (var i = 0; i < 6; i++) { p += direction * step_size; optical += density(p) * step_size; step_size *= 1.85; }
    return optical;
}
@fragment fn cloud_main(in: VertexOut) -> @location(0) vec4<f32> {
    let rd = ray(in.uv, u.resolution.w);
    if u.weather.x <= 0.001 && u.air.x <= 0.01 { return vec4(0.0, 0.0, 0.0, 1.0); }
    if rd.y <= 0.005 { return vec4(0.0, 0.0, 0.0, 1.0); }
    let origin = vec3(0.0, 0.045, 0.0);
    let lo = (cloud_base() - origin.y) / rd.y;
    let hi = min(45.0, (cloud_top() - origin.y) / rd.y);
    let ds = (hi - lo) / 88.0;
    var distance = lo + 0.5 * ds; var sum = vec3(0.0); var transmission = 1.0;
    let day = smoothstep(-0.12, 0.10, u.sun.y);
    let warm = exp(-pow((u.sun.y - 0.03) * 4.5, 2.0));
    let light_dir = light_direction();
    let sun_color = mix(vec3(1.0, 0.96, 0.87), vec3(1.0, 0.36, 0.105), warm * 0.88);
    let moon_strength = u.sun.w * smoothstep(-0.02, 0.10, u.moon.y) * 0.06 * (1.0 - day);
    let solar = day * (1.12 + warm * 0.72) + moon_strength;
    let source_color = mix(vec3(0.55, 0.70, 1.0), sun_color, day);
    let mu = dot(rd, light_dir);
    let phase = 0.26 + 0.075 * phase_hg(mu, 0.68) + 0.10 * phase_hg(mu, -0.25);
    var ambient = mix(vec3(0.002, 0.004, 0.011), vec3(0.255, 0.354, 0.49), day);
    ambient = mix(ambient, vec3(0.25, 0.20, 0.27), warm * 0.35 * day);
    for (var i = 0; i < 88; i++) {
        if transmission < 0.014 { transmission = 0.0; break; }
        if distance > hi || hi <= lo { break; }
        let p = origin + rd * distance; let d = density(p);
        if d > 0.004 {
            let optical = shadow(p, light_dir);
            let sun_t = exp(-optical * 3.4); let multiple = exp(-optical * 0.65) * 0.17;
            let h = sat((p.y - cloud_base()) / 3.5);
            var light = ambient * (0.31 + 0.51 * h) + source_color * solar * (sun_t * phase * 2.45 + multiple);
            light *= mix(1.0, 0.72, rain());
            light *= mix(1.0, 0.72 + 0.48 * n3(vec3(p.xz * 0.17, 0.15)) + 0.18 * noise(p * 0.24).g, u.weather.w);
            let absorb = 1.0 - exp(-d * ds * 5.2);
            let haze = 1.0 - exp(-distance * (0.021 + u.air.y * 0.05));
            light = mix(light, sky_color(rd) * 0.84, haze * 0.52);
            sum += transmission * absorb * light; transmission *= 1.0 - absorb;
        }
        distance += ds;
    }
    return vec4(sum, transmission);
}
fn upper_layers(rd: vec3<f32>, background: vec3<f32>) -> vec4<f32> {
    var sky = background; var transmission = 1.0;
    let day = smoothstep(-0.11, 0.13, u.sun.y); let warm = exp(-pow(u.sun.y * 5.0, 2.0));
    let tint = mix(vec3(0.85, 0.91, 1.0), vec3(1.0, 0.54, 0.29), warm * 0.80) * (0.018 + 0.95 * day);
    if u.weather.z > 0.001 {
        var p = rd.xz / max(0.12, rd.y);
        p -= u.wind_high.zw * 0.13;
        p = mat2x2<f32>(vec2(0.94, -0.342), vec2(0.342, 0.94)) * p;
        let curl = n3(vec3(p * 0.062, 0.23)) - 0.5;
        let veil = n3(vec3(p.x * 0.058 + 0.37, p.y * 0.32 + curl * 0.09, 0.71));
        let strand = n3(vec3(p.x * 0.14 + 0.12, p.y * 2.1 + curl * 0.30, 0.49));
        let fine = n3(vec3(p.x * 0.29, p.y * 5.9 + curl * 0.46, 0.18));
        let c = smoothstep(0.40, 0.61, veil + 0.24 * (strand - 0.5) + 0.075 * (fine - 0.5))
            * smoothstep(0.16, 0.39, rd.y) * u.weather.z * 0.78;
        sky = mix(sky, tint, c); transmission *= 1.0 - c;
    }
    if u.weather.y > 0.001 {
        // Thin altocumulus/altostratus at a separate height and wind.
        let p = rd.xz / max(0.08, rd.y) * 5.8 - u.wind_mid.zw;
        let a = noise(vec3(p * 0.052, 0.34)); let b = noise(vec3(p * 0.187, 0.63));
        let sheet = a.r * 0.70 + b.g * 0.30;
        let d = smoothstep(0.66 - u.weather.y * 0.40, 0.81 - u.weather.y * 0.40, sheet);
        let alpha = d * u.weather.y * 0.88;
        let lit = tint * (0.54 + b.b * 0.30);
        sky = mix(sky, lit, alpha); transmission *= 1.0 - alpha;
    }
    return vec4(sky, transmission);
}
fn moon_disc(rd: vec3<f32>) -> vec3<f32> {
    if u.moon.y < -0.01 { return vec3(0.0); }
    let delta = rd - u.moon.xyz * dot(rd, u.moon.xyz);
    let radius = 0.00465;
    let q = length(delta) / radius;
    if q > 1.08 || dot(rd, u.moon.xyz) < 0.0 { return vec3(0.0); }
    let z = sqrt(max(0.0, 1.0 - q * q));
    var tangent = u.sun.xyz - u.moon.xyz * dot(u.sun.xyz, u.moon.xyz);
    if length(tangent) < 0.0001 { tangent = u.right.xyz * u.moon.w; }
    tangent = normalize(tangent);
    let phase_z = 2.0 * u.sun.w - 1.0;
    let normal_light = dot(delta / radius, tangent) * sqrt(max(0.0, 1.0 - phase_z * phase_z)) + z * phase_z;
    let lit = smoothstep(-0.018, 0.020, normal_light) * (1.0 - smoothstep(0.94, 1.04, q));
    return vec3(0.70, 0.75, 0.83) * lit;
}
fn hash(p: vec2<f32>) -> f32 { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
@fragment fn present_main(in: VertexOut) -> @location(0) vec4<f32> {
    let rd = ray(in.uv, 1.0);
    let upper = upper_layers(rd, sky_color(rd));
    let drift = u.wind_low.zw - u.cache_wind.xy;
    let point = rd * u.cache.y / max(0.02, rd.y) - vec3(drift.x, 0.0, drift.y);
    let old_ray = normalize(point);
    let z = max(0.01, dot(old_ray, u.forward.xyz));
    let old_xy = vec2(dot(old_ray, u.right.xyz) / u.resolution.z, dot(old_ray, u.up.xyz)) / (z * u.right.w * u.resolution.w);
    // Cloud render target follows WGPU upper-left texture coordinates.
    let old_uv = vec2(old_xy.x * 0.5 + 0.5, 0.5 - old_xy.y * 0.5);
    let cloud = textureSampleLevel(cloud_texture, cloud_sampler, old_uv, 0.0);
    let T = cloud.a * upper.a;
    let disk = smoothstep(cos(0.0049), cos(0.0040), dot(rd, u.sun.xyz)) * smoothstep(-0.15, 0.12, u.sun.y);
    var col = cloud.rgb + upper.rgb * cloud.a + vec3(6.0, 4.5, 2.8) * disk * T * smoothstep(0.035, 0.20, T);
    col += moon_disc(rd) * T;
    let night = 1.0 - smoothstep(-0.15, -0.035, u.sun.y);
    if night > 0.001 {
        // The legacy decorative night points remain stable in world direction.
        let angles = vec2(atan2(rd.x, rd.z), asin(clamp(rd.y, -1.0, 1.0)));
        let cell = floor(angles * 240.0);
        let point_star = smoothstep(0.999, 0.9998, hash(cell + u.cache.z));
        let local = fract(angles * 240.0) - 0.5;
        col += vec3(point_star * exp(-dot(local, local) * 45.0) * night * T * 0.30);
    }
    let bank = rain() * smoothstep(0.05, 0.35, rd.x) * pow(1.0 - sat(rd.y), 3.0);
    col = mix(col, vec3(0.20, 0.26, 0.32) * (0.28 + 0.72 * smoothstep(-0.12, 0.10, u.sun.y)), bank * 0.32);
    // Extinction hides the finite volume's 45 km boundary at shallow angles.
    // Otherwise a view reaching the horizon exposes a hard cache edge/streaks.
    let horizon_start = clamp((cloud_base() - 0.045) / 45.0 + 0.015, 0.025, 0.16);
    let horizon_visibility = smoothstep(horizon_start, horizon_start + 0.045, rd.y);
    let air_day = smoothstep(-0.12, 0.10, u.sun.y);
    let horizon_air = mix(sky_color(rd), vec3(0.44, 0.51, 0.59) * (0.24 + 0.70 * air_day), u.weather.w * 0.5 + rain() * 0.3);
    col = mix(horizon_air, col, horizon_visibility);
    col = pow(vec3(1.0) - exp(-max(col, vec3(0.0)) * 1.23), vec3(0.454545));
    col += (hash(in.position.xy + 13.7) - 0.5) / 350.0;
    if u.protection.x > 0.0 && u.reading.z > 0.0 && u.reading.w > 0.0 {
        let uv = vec2(in.uv.x, 1.0 - in.uv.y);
        let center = u.reading.xy + u.reading.zw * 0.5;
        let radius = max(u.reading.zw * 0.60, vec2(0.08, 0.06));
        let d = (uv - center) / radius;
        let veil = u.protection.x * exp(-dot(d, d) * 0.65);
        let tone = select(vec3(1.0), vec3(0.025, 0.035, 0.055), u.sun.y < -0.05);
        col = mix(col, tone, veil);
    }
    return vec4(col, 1.0);
}
