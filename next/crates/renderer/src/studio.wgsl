struct Material { color: vec4<f32>, params: vec4<f32>, emission: vec4<f32>, coat: vec4<f32> }
@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var tex_sampler: sampler;
@group(1) @binding(2) var base_tex: texture_2d<f32>;
@group(1) @binding(3) var normal_tex: texture_2d<f32>;
@group(1) @binding(4) var ao_tex: texture_2d<f32>;
@group(1) @binding(5) var mr_tex: texture_2d<f32>;
struct Projection { u:vec4<f32>, v:vec4<f32>, normal:vec4<f32> }
@group(2) @binding(0) var<uniform> projection: Projection;
@group(2) @binding(1) var artwork: texture_2d<f32>;
@group(2) @binding(2) var artwork_sampler: sampler;
struct Input { @location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) uv: vec2<f32>, @location(3) uv1: vec2<f32> }
struct Output { @builtin(position) clip: vec4<f32>, @location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) uv: vec2<f32>, @location(3) uv1: vec2<f32> }
@vertex fn vs(v: Input) -> Output {
 var o: Output; o.clip=camera.vp*vec4(v.p,1.);o.p=v.p;o.n=v.n;o.uv=v.uv;o.uv1=v.uv1;return o;
}
fn fresnel(c: f32, f0: vec3<f32>) -> vec3<f32> {return f0+(vec3(1.)-f0)*pow(clamp(1.-c,0.,1.),5.);}
fn light(n: vec3<f32>,v: vec3<f32>,l: vec3<f32>,radiance: vec3<f32>,base: vec3<f32>,metal: f32,rough: f32) -> vec3<f32> {
 let h=normalize(v+l);let nv=max(dot(n,v),0.001);let nl=max(dot(n,l),0.);let nh=max(dot(n,h),0.);let vh=max(dot(v,h),0.);
 let a=rough*rough;let a2=a*a;let d=a2/(3.14159265*pow(nh*nh*(a2-1.)+1.,2.)+0.0001);
 let k=pow(rough+1.,2.)/8.;let g=nv/(nv*(1.-k)+k)*nl/(nl*(1.-k)+k);
 let f=fresnel(vh,mix(vec3(0.04),base,metal));
 let spec=d*g*f/max(4.*nv*nl,0.001);
 let coat=material.coat.x*pow(max(dot(n,h),0.),mix(180.,25.,material.coat.y))*0.22;
 return ((vec3(1.)-f)*(1.-metal)*base/3.14159265+spec+vec3(coat))*radiance*nl;
}
@fragment fn fs(o: Output,@builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
 var base=textureSample(base_tex,tex_sampler,o.uv)*material.color;
 let projected=vec2(dot(vec4(o.p,1.),projection.u),dot(vec4(o.p,1.),projection.v));
 let ink=textureSampleLevel(artwork,artwork_sampler,projected,0.);
 var ink_alpha=0.;
 if projection.normal.w>0.5 && dot(normalize(o.n),projection.normal.xyz)>0.15 && all(projected>=vec2(0.)) && all(projected<=vec2(1.)) {
   ink_alpha=ink.a;
   base=vec4(mix(base.rgb,ink.rgb,ink.a),base.a);
 }
 if material.params.z>=0. && base.a<material.params.z {discard;}
 let nn=normalize(select(-o.n,o.n,front));
 let dp1=dpdx(o.p);let dp2=dpdy(o.p);let duv1=dpdx(o.uv);let duv2=dpdy(o.uv);
 let tangent=dp1*duv2.y-dp2*duv1.y;
 let det=duv1.x*duv2.y-duv1.y*duv2.x;
 var n=nn;
 let map_n=textureSample(normal_tex,tex_sampler,o.uv).xyz*2.-vec3(1.);
 if length(tangent)>0.000001 && abs(det)>0.0000001 {
   let t=normalize(tangent*sign(det));let b=normalize(cross(nn,t))*sign(det);
   n=normalize(t*map_n.x*material.params.w+b*map_n.y*material.params.w+nn*map_n.z);
 }
 let v=normalize(camera.eye.xyz-o.p);let mr=textureSample(mr_tex,tex_sampler,o.uv);
 let metal=mix(clamp(material.params.x*mr.b,0.,1.),0.,ink_alpha);let rough=mix(clamp(material.params.y*mr.g,0.065,1.),0.48,ink_alpha);
 let auv=select(o.uv,o.uv1,material.coat.z>0.5);
 let ao=mix(1.,textureSample(ao_tex,tex_sampler,auv).r,material.emission.w);
 let day=camera.environment.x;
 var color=base.rgb*(0.06+day*(0.12+0.22*max(n.y,0.)))*(1.-metal*0.65)*ao;
 color+=light(n,v,camera.sun.xyz,sunlight()*camera.sun.w*3.8,base.rgb,metal,rough);
 color+=light(n,v,normalize(vec3(4.,3.,-4.)),vec3(2.6,2.8,3.1)*(0.14+day*0.86),base.rgb,metal,rough);
 color+=light(n,v,normalize(vec3(0.,2.,5.)),vec3(1.1)*(0.15+day*0.85),base.rgb,metal,rough);
 // Analytic sky reflection plus soft studio fill; no HDR convolution.
 let r=reflect(-v,n);let strip=pow(max(1.-abs(r.y-0.6),0.),mix(26.,3.,rough));
 let environment=sky_color(r)*(0.3+day*0.7)+vec3(strip*0.5*(0.15+day*0.85));
 color+=environment*fresnel(max(dot(n,v),0.),mix(vec3(0.04),base.rgb,metal))*ao;
 color+=material.emission.rgb;
 // ACES approximation, output remains linear for the sRGB render target.
 color=clamp((color*(2.51*color+vec3(0.03)))/(color*(2.43*color+vec3(0.59))+vec3(0.14)),vec3(0.),vec3(1.));
 return vec4(color,base.a);
}
