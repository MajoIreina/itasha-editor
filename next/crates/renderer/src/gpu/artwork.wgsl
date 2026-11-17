
struct Art {u:vec4<f32>,v:vec4<f32>,n:vec4<f32>,placement:vec4<f32>,rotation:vec4<f32>,flags:vec4<f32>};
@group(1) @binding(0) var<uniform> art: Art;
@group(1) @binding(1) var image: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;
struct Out {@builtin(position) position:vec4<f32>, @location(0) world:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) image_uv:vec2<f32>};
@vertex fn vs(@location(0) p:vec3<f32>, @location(1) n:vec3<f32>, @location(2) uv:vec2<f32>) -> Out {
 var o:Out; o.position=camera.vp*vec4(p,1.);o.world=p;o.normal=n;o.image_uv=uv;return o;
}
fn alpha_at(uv:vec2<f32>) -> f32 {if any(uv < vec2(0.)) || any(uv > vec2(1.)) {return 0.;} return textureSampleLevel(image,samp,uv,0.).a;}
@fragment fn fs(i:Out) -> @location(0) vec4<f32> {
 let n=normalize(i.normal);var uv=i.image_uv;if art.rotation.z>.5 {uv.x=1.-uv.x;}
 let texel=max(fwidth(uv),vec2(.00001));
 if any(uv < -texel*3.) || any(uv > vec2(1.)+texel*3.) {discard;}
 let a=alpha_at(uv);var color=textureSampleLevel(image,samp,clamp(uv,vec2(0.),vec2(1.)),0.);
 color.a=a*art.flags.y;
 let edge=max(max(alpha_at(uv+vec2(texel.x*2.,0.)),alpha_at(uv-vec2(texel.x*2.,0.))),max(alpha_at(uv+vec2(0.,texel.y*2.)),alpha_at(uv-vec2(0.,texel.y*2.))));
 let inner=min(min(alpha_at(uv+vec2(texel.x*2.,0.)),alpha_at(uv-vec2(texel.x*2.,0.))),min(alpha_at(uv+vec2(0.,texel.y*2.)),alpha_at(uv-vec2(0.,texel.y*2.))));
 if art.flags.z > .5 && edge-inner > .2 {return vec4(.02,.45,1.,max(color.a,.95));}
 if color.a < .002 {discard;}
 let day=camera.environment.x;
 let illumination=vec3(.06+day*(.34+.18*max(n.y,0.)))+sunlight()*camera.sun.w*max(dot(n,camera.sun.xyz),0.)*.6;
 return vec4(color.rgb*illumination,color.a);
}
