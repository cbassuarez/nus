// Native port of the recovered bivalent-renderer.js and constellations-renderer.js.
// The source uses implicit texture gradients in the analytic sphere branches.
// Keep those sampling semantics, rather than replacing the images with noise.
diagnostic(off, derivative_uniformity);
struct Uniforms {
    resolution: vec4<f32>, // output width/height, dust width/height
    view: vec4<f32>,       // phase, blend, exposure, local motion time
    options: vec4<f32>,    // seed, line strength, reserved
    reading: vec4<f32>,    // actual Home reading rectangle, top-left normalized
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var dust_tex: texture_2d<f32>;
@group(0) @binding(2) var earth_tex: texture_2d<f32>;
@group(0) @binding(3) var detail_tex: texture_2d<f32>;
@group(0) @binding(4) var clouds_tex: texture_2d<f32>;
@group(0) @binding(5) var cloud_detail_tex: texture_2d<f32>;
@group(0) @binding(6) var map_sampler: sampler;
@group(0) @binding(7) var dust_sampler: sampler;
@group(0) @binding(8) var cloud_sampler: sampler;
const PI: f32 = 3.14159265359;
struct Fullscreen { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> Fullscreen {
    let p = array<vec2<f32>, 3>(vec2(-1., -1.), vec2(3., -1.), vec2(-1., 3.))[i];
    var o: Fullscreen; o.position = vec4(p, 0., 1.); o.uv = vec2(p.x*.5+.5, .5-p.y*.5); return o;
}
fn sky_hash(p0: vec2<f32>) -> f32 {
    var p = fract(p0*vec2(123.34,456.21)); p += vec2(dot(p,p+vec2(45.32))); return fract(p.x*p.y);
}
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p); var f = fract(p); f = f*f*(vec2(3.)-2.*f);
    return mix(mix(sky_hash(i),sky_hash(i+vec2(1.,0.)),f.x),mix(sky_hash(i+vec2(0.,1.)),sky_hash(i+vec2(1.,1.)),f.x),f.y);
}
fn fbm(p0: vec2<f32>) -> f32 {
    var p=p0; var s=0.; var a=.5;
    for(var i=0u;i<5u;i++){s+=a*noise(p);p=mat2x2<f32>(vec2(.80,-.60),vec2(.60,.80))*p*2.07+vec2(19.8,8.3);a*=.51;}
    return s;
}
fn field(p: vec2<f32>,depth: f32) -> f32 {
    let q=p+vec2(depth*.41,depth*.23)+vec2(u.options.x*1.731,u.options.x*.823);
    let warp=vec2(fbm(q*1.7+vec2(3.7)),fbm(q*1.7+vec2(21.2)));
    return fbm(q*4.+warp*2.5);
}
@fragment fn dust_main(in: Fullscreen) -> @location(0) vec4<f32> {
    let uv=vec2(in.uv.x,1.-in.uv.y);
    let p=(uv-vec2(.5))*vec2(u.resolution.x/u.resolution.y,1.)*1.35;
    var radiance=vec3(0.); var transmission=1.; let ambient=vec3(.00065,.0012,.0020);
    for(var i=0u;i<6u;i++){
        let z=f32(i)/5.;let s=p*(1.+z*.19);let f=field(s,z);
        let large=fbm(s*1.6+vec2(4.3+u.options.x*.19,7.9+z*.8));
        let fine=fbm(s*37.+vec2(2.8+z*.47,u.options.x*.1));
        let boundary=.07+.11*sin(s.y*5.8+1.1)+.09*sin(s.y*11.3+.2);
        let body=smoothstep(boundary-.25,boundary+.18,s.x+(large-.5)*.7);
        let ragged=smoothstep(.25,.76,f+(fine-.5)*.15);
        let filament=pow(max(0.,1.-abs(f-.48)*5.),3.);
        let core=smoothstep(.42,.66,fbm(s*3.1+vec2(24.+u.options.x*.08,z*.5)));
        let density=body*(ragged*1.48+filament*.23)+body*core*.55;
        let absorb=1.-exp(-density*.66);
        let bright_side=field(s+vec2(-.045,.04),z)-f;
        let silver=clamp(.24+bright_side*3.2+(fine-.5)*.66,.025,.77);
        let q=(s-vec2(.55,.35))*vec2(1.65,2.2);let reflected=exp(-dot(q,q));
        var illumination=mix(vec3(.120,.081,.051),vec3(.099,.131,.176),reflected*.63);
        illumination*=silver*exp(-core*1.4);
        illumination+=vec3(.020,.030,.049)*reflected*max(bright_side,0.)*3.;
        radiance+=transmission*absorb*illumination;transmission*=1.-absorb;
    }
    let distant=fbm(p*5.1+vec2(36.+u.options.x*.19,62.));
    radiance+=transmission*(ambient+vec3(.003,.004,.006)*pow(distant,3.));
    return vec4(sqrt(max(radiance,vec3(0.))),transmission);
}
fn gl_pixel(p: vec2<f32>) -> vec2<f32> { return vec2(p.x,u.resolution.y-p.y); }
fn planet_center() -> vec3<f32> {
    let ph=u.view.x-.5;let b=u.view.y;return vec3(1.18364+ph*.12+b*.70,-2.40377+ph*.06-b*.75,-1.);
}
fn sphere(rd: vec3<f32>,c: vec3<f32>,r: f32) -> vec2<f32> {
    let b=dot(rd,c);let disc=b*b-dot(c,c)+r*r;if(disc<0.){return vec2(-1.);}let h=sqrt(disc);return vec2(b-h,b+h);
}
fn tonemap(c: vec3<f32>) -> vec3<f32> {return clamp((c*(2.51*c+vec3(.03)))/(c*(2.43*c+vec3(.59))+vec3(.14)),vec3(0.),vec3(1.));}
fn sky_offset() -> vec2<f32> {
    let aspect=u.resolution.x/u.resolution.y;
    let ph=u.view.x-.5;let b=u.view.y;
    return vec2((ph*.10+(1.-b)*-.12)*min(1.,aspect)/aspect,ph*.04+(1.-b)*.045);
}
fn sky_color(pixel: vec2<f32>) -> vec3<f32> {
    let uv=pixel/u.resolution.xy;
    let dust_uv=(uv+sky_offset()-vec2(.5))/1.35+vec2(.5);
    // Render textures have top-left sampling in wgpu; source GLSL used bottom-left.
    let matter=textureSampleLevel(dust_tex,dust_sampler,vec2(dust_uv.x,1.-dust_uv.y),0.);
    let adaptation=exp2(-11.*(1.-smoothstep(.05,.85,u.view.y)));
    let grain=(sky_hash(pixel+vec2(u.options.x))-.5)*.00048*adaptation;
    let c=max(vec3(0.),matter.rgb*matter.rgb*u.view.z*1.5*adaptation+vec3(grain));
    return pow(vec3(1.)-exp(-c*1.8),vec3(.56))+vec3(.003,.004,.006)*(1.-adaptation);
}
fn geographic_normal(n: vec3<f32>) -> vec3<f32> {
    let origin=normalize(vec3(-.43,.88,.2));let east=normalize(vec3(.2,0.,.43));let north=normalize(cross(origin,east));
    let lat=radians(37.);let lon=radians(12.)+(u.view.x-.5)*.10;
    let normal_geo=vec3(cos(lat)*cos(lon),sin(lat),cos(lat)*sin(lon));
    let east_geo=vec3(-sin(lon),0.,cos(lon));let north_geo=vec3(-sin(lat)*cos(lon),cos(lat),-sin(lat)*sin(lon));
    return normalize(normal_geo*dot(n,origin)+east_geo*dot(n,east)+north_geo*dot(n,north));
}
fn surface_sample(geo: vec3<f32>) -> vec3<f32> {
    let lon=atan2(geo.z,geo.x);let lat=asin(clamp(geo.y,-1.,1.));
    let uv=vec2(lon/(2.*PI)+.5,.5-lat/PI);var base=textureSample(earth_tex,map_sampler,uv).rgb;
    let detail=vec2((degrees(lon)+12.)/57.,(56.-degrees(lat))/36.);
    if(all(detail>vec2(0.))&&all(detail<vec2(1.))){
        let edge=min(detail,vec2(1.)-detail);let weight=smoothstep(0.,.035,min(edge.x,edge.y));
        base=mix(base,textureSample(detail_tex,map_sampler,detail).rgb,weight);
    }return base;
}
fn cloud_uv(n: vec3<f32>) -> vec2<f32> {
    let geo=geographic_normal(n);let lon=atan2(geo.z,geo.x)+radians(-16.)-u.view.w*.000035;
    return vec2(fract(lon/(2.*PI)+.5),.5-asin(clamp(geo.y,-1.,1.))/PI);
}
fn cloud_sample(uv: vec2<f32>) -> f32 {
    let detail=vec2((uv.x*360.-180.+45.)/90.,(56.-(90.-uv.y*180.))/36.);
    var value=textureSample(clouds_tex,cloud_sampler,uv).r;
    if(all(detail>vec2(0.))&&all(detail<vec2(1.))){
        let edge=min(detail,vec2(1.)-detail);let weight=smoothstep(0.,.04,min(edge.x,edge.y));
        value=mix(value,textureSample(cloud_detail_tex,cloud_sampler,detail).r,weight);
    }return pow(clamp((value-.055)/.945,0.,1.),1.24);
}
fn cloud_layer(rd: vec3<f32>,center: vec3<f32>,radius: f32,sun: vec3<f32>) -> vec4<f32> {
    let hit=sphere(rd,center,radius*1.0014);if(hit.x<0.){return vec4(0.);}
    let n=normalize(rd*hit.x-center);let uv=cloud_uv(n);let density=cloud_sample(uv);
    let relief=clamp((density-cloud_sample(uv+vec2(.00045,-.00028)))*1.15,-.20,.20);
    let day=smoothstep(-.06,.15,dot(n,sun));let diffuse=max(0.,dot(n,sun));let facing=clamp(dot(n,-rd),0.,1.);
    let alpha=(1.-exp(-density*2.))*.87*pow(max(facing,.15),-.08)*smoothstep(0.,.028,facing);
    let white=mix(vec3(.49,.57,.68),vec3(.94,.96,1.),clamp(.73+relief,0.,1.));
    var light=white*(.045+day*(.40+diffuse*.70))*(.91+relief);
    light=mix(light,vec3(.038,.14,.31)*day,pow(1.-facing,7.)*.34);
    return vec4(light,clamp(alpha,0.,.94));
}
fn cloud_shadow(n: vec3<f32>,sun: vec3<f32>,radius: f32) -> f32 {
    let cloud_radius=radius*1.0014;let origin=n*radius;let along=dot(origin,sun);
    let distance=-along+sqrt(max(0.,along*along+cloud_radius*cloud_radius-radius*radius));
    let uv=cloud_uv(normalize(origin+sun*distance));
    let density=(cloud_sample(uv+vec2(.00025,.00015))+cloud_sample(uv-vec2(.00025,.00015)))*.5;
    return 1.-exp(-density*1.7);
}
fn grain_hash(p: vec2<f32>) -> f32 {
    var q=fract(vec3(p.x,p.y,p.x)*.1031);q+=vec3(dot(q,q.yzx+vec3(33.33)));return fract((q.x+q.y)*q.z);
}
// The prototype's edge gradient is artwork composition, not its reading shade.
// Composite after stars in the same render pass, matching the source CSS layer.
fn edge_alpha(pixel_top: vec2<f32>) -> f32 {
    let y=clamp(pixel_top.y/u.resolution.y,0.,1.);
    return .08*(1.-clamp(y/.20,0.,1.))+.75*clamp((y-.70)/.30,0.,1.);
}
@fragment fn edge_fs(in: Fullscreen) -> @location(0) vec4<f32> {
    return vec4(vec3(3.,6.,8.)/255.,edge_alpha(in.position.xy));
}
@fragment fn scene_main(in: Fullscreen) -> @location(0) vec4<f32> {
    let pixel=gl_pixel(in.position.xy);if(u.view.y>.999){return vec4(sky_color(pixel),1.);}
    let uv=(pixel-.5*u.resolution.xy)/u.resolution.y;let rd=normalize(vec3(uv,-2.));
    let center=planet_center();let radius=2.67939;let ph=u.view.x-.5;
    let sun=normalize(vec3(.28-ph*.16,.72+ph*.08,.63));let hit=sphere(rd,center,radius);
    let projection=dot(rd,center);let closest=sqrt(max(0.,dot(center,center)-projection*projection));
    let altitude=(closest-radius)/radius;let near_normal=normalize(rd*projection-center);
    let horizon_light=smoothstep(-.055,.55,dot(near_normal,sun));
    if(hit.x<0.){
        let shell=exp(-max(altitude,0.)/.00145);let outer=exp(-max(altitude,0.)/.0048);
        var atmosphere=(vec3(.025,.25,.82)*shell*.60+vec3(.008,.053,.19)*outer*.34)*horizon_light;
        atmosphere=pow(tonemap(atmosphere*u.view.z*1.4),vec3(1./2.2));
        let clouds=cloud_layer(rd,center,radius,sun);let cloud_light=pow(tonemap(clouds.rgb*u.view.z*1.25),vec3(1./2.2));
        let background=mix(sky_color(pixel)+atmosphere,cloud_light+atmosphere*.38,clouds.a);
        return vec4(clamp(background,vec3(0.),vec3(1.)),1.);
    }
    let n=normalize(rd*hit.x-center);let srgb=surface_sample(geographic_normal(n));let albedo=pow(srgb,vec3(2.2));
    let day=smoothstep(-.06,.15,dot(n,sun));let diffuse=max(dot(n,sun),0.);
    let water=smoothstep(.008,.055,srgb.b-max(srgb.r,srgb.g));let facing=clamp(dot(n,-rd),0.,1.);
    var color=albedo*(.025+day*(.50+diffuse*.85));let half_v=normalize(sun-rd);
    let specular=pow(max(dot(n,half_v),0.),170.);let fresnel=.025+.975*pow(1.-facing,5.);
    color+=vec3(1.,.88,.69)*specular*fresnel*.45*water*day;color*=1.-cloud_shadow(n,sun,radius)*.43*day;
    let clouds=cloud_layer(rd,center,radius,sun);color=mix(color,clouds.rgb,clouds.a);
    color=mix(color,vec3(.018,.10,.24)*day,pow(1.-facing,7.5)*.42);
    color+=vec3(.025,.19,.58)*pow(1.-facing,22.5)*horizon_light*.25;
    let edge=smoothstep(0.,1.8/u.resolution.y,max(0.,-altitude));color=mix(vec3(.023,.18,.52)*horizon_light,color,edge);
    color=pow(tonemap(color*u.view.z*1.25),vec3(1./2.2));color+=vec3((grain_hash(pixel+vec2(u.options.x))-.5)/650.);
    return vec4(clamp(color,vec3(0.),vec3(1.)),1.);
}
// Catalogue directions use the source's ICRS Orion camera, not a local sky.
fn project(d: vec3<f32>) -> vec3<f32> {
    let ra=radians(82.5);let dec=radians(4.);
    let forward=vec3(cos(dec)*cos(ra),cos(dec)*sin(ra),sin(dec));
    let right=vec3(sin(ra),-cos(ra),0.);let up=vec3(-sin(dec)*cos(ra),-sin(dec)*sin(ra),cos(dec));
    return vec3(dot(d,right),dot(d,up),dot(d,forward));
}
fn projected_pixel(q: vec3<f32>) -> vec2<f32> {
    let ndc=vec2(q.x/(u.resolution.x/u.resolution.y),q.y)*4./max(q.z,.01)-2.*sky_offset();
    return vec2((ndc.x+1.)*.5,(1.-ndc.y)*.5)*u.resolution.xy;
}
fn clip_pixel(p: vec2<f32>) -> vec4<f32> {return vec4(p/u.resolution.xy*vec2(2.,-2.)+vec2(-1.,1.),0.,1.);}
fn visible(pixel_top: vec2<f32>) -> f32 {
    if(u.view.y>.999){return 1.;}let rd=normalize(vec3((gl_pixel(pixel_top)-.5*u.resolution.xy)/u.resolution.y,-2.));
    let c=planet_center();let b=dot(rd,c);let disc=b*b-dot(c,c)+2.67939*2.67939;
    if(disc>=0.&&b-sqrt(max(disc,0.))>0.){return 0.;}
    let near=sqrt(max(0.,dot(c,c)-b*b))-2.67939;return select(1.,smoothstep(0.,.035,near),b>0.);
}
fn quiet(pixel_top: vec2<f32>) -> f32 {
    if(u.reading.z<=0.||u.reading.w<=0.){return 1.;}
    let q=(pixel_top/u.resolution.xy-(u.reading.xy+.5*u.reading.zw))/max(.5*u.reading.zw,vec2(.001));
    return 1.-.74*exp(-dot(q,q)*1.7);
}
struct StarIn { @location(0) direction: vec3<f32>, @location(1) magnitude: f32, @location(2) bv: f32 };
struct StarOut { @builtin(position) position: vec4<f32>, @location(0) point: vec2<f32>, @location(1) @interpolate(flat) color: vec3<f32>, @location(2) @interpolate(flat) light: f32, @location(3) @interpolate(flat) radius: f32 };
fn corner(i: u32) -> vec2<f32> {return array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.))[i];}
@vertex fn star_vs(s: StarIn,@builtin(vertex_index) i: u32) -> StarOut {
    let q=project(s.direction);let flux=pow(10.,-.4*(s.magnitude-1.));let pixel=clamp(u.resolution.y/760.,.75,1.5);
    let radius=max(.72,(.46+min(1.35,pow(flux,.31)*.52))*pixel);let size=clamp(radius*8.,3.,20.);
    var o: StarOut;o.point=corner(i);o.position=clip_pixel(projected_pixel(q)+(o.point-vec2(.5))*size);o.radius=radius/size;
    let adaptation=.55+.45*smoothstep(.05,.85,u.view.y);
    o.light=clamp(pow(flux,.33)*2.60,.12,3.2)*pow(clamp(u.view.z,.4,2.5),.65)*adaptation;
    o.color=mix(vec3(.63,.77,1.),vec3(.97,.98,1.),smoothstep(-.3,.35,s.bv));
    o.color=mix(o.color,vec3(1.,.76,.49),smoothstep(.35,1.5,s.bv));
    if(q.z<=.01){o.position=vec4(-2.,-2.,0.,1.);o.light=0.;}return o;
}
@fragment fn star_fs(in: StarOut) -> @location(0) vec4<f32> {
    let vis=visible(in.position.xy);if(vis<.001){discard;}
    let d=length(in.point-vec2(.5))/in.radius;let profile=exp(-d*d*1.15)+.035*exp(-d*d*.16);
    return vec4(in.color*profile*in.light*vis*quiet(in.position.xy),0.);
}
struct LineIn { @location(0) a: vec3<f32>, @location(1) b: vec3<f32> };
struct LineOut { @builtin(position) position: vec4<f32> };
@vertex fn line_vs(s: LineIn,@builtin(vertex_index) i: u32) -> LineOut {
    var a=project(s.a);var b=project(s.b);var o: LineOut;
    if(a.z<=.01&&b.z<=.01){o.position=vec4(-2.,-2.,0.,1.);return o;}
    if(a.z<.01){a=mix(a,b,(.01-a.z)/(b.z-a.z));}if(b.z<.01){b=mix(b,a,(.01-b.z)/(a.z-b.z));}
    let pa=projected_pixel(a);let pb=projected_pixel(b);let d=pb-pa;let length_=max(length(d),.0001);let normal=vec2(-d.y,d.x)/length_;
    let c=corner(i);o.position=clip_pixel(mix(pa,pb,c.x)+normal*(c.y-.5));return o;
}
@fragment fn line_fs(in: LineOut) -> @location(0) vec4<f32> {
    let a=.17*u.options.y*(.035+.965*smoothstep(.15,.90,u.view.y))*visible(in.position.xy)*quiet(in.position.xy);
    if(a<.001){discard;}return vec4(vec3(.65,.71,.77)*a,0.);
}
