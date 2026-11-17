@group(0) @binding(0) var scene:texture_2d<f32>;
@group(0) @binding(1) var tex_sampler:sampler;
struct Params { texel_mode: vec4<f32>, output_pixel: vec4<f32> }
@group(0) @binding(2) var<uniform> params:Params;
struct Out { @builtin(position) p:vec4<f32>, @location(0) uv:vec2<f32> }
@vertex fn vs(@builtin(vertex_index) index:u32)->Out {
 let uv=vec2(f32((index<<1u)&2u),f32(index&2u));var o:Out;o.p=vec4(uv*2.-1.,0.,1.);o.uv=vec2(uv.x,1.-uv.y);return o;
}
fn sample_color(uv:vec2<f32>)->vec3<f32> {return textureSampleLevel(scene,tex_sampler,uv,0.).rgb;}
fn luma(c:vec3<f32>)->f32 {return sqrt(max(dot(c,vec3(0.299,0.587,0.114)),0.));}
@fragment fn fs(o:Out)->@location(0) vec4<f32> {
 let c=sample_color(o.uv);if params.texel_mode.z<0.5 {return vec4(c,1.);}
 if params.texel_mode.z>1.5 {
   // Four area samples in linear color. At 2x resolution each tap lands on
   // one source pixel; the sRGB output conversion happens after averaging.
   let d=params.output_pixel.xy*0.25;
   return vec4((sample_color(o.uv+vec2(-d.x,-d.y))+sample_color(o.uv+vec2(d.x,-d.y))+
                sample_color(o.uv+vec2(-d.x,d.y))+sample_color(o.uv+d))*0.25,1.);
 }
 let px=params.texel_mode.xy;
 let nw=luma(sample_color(o.uv+vec2(-1.,-1.)*px));
 let ne=luma(sample_color(o.uv+vec2(1.,-1.)*px));
 let sw=luma(sample_color(o.uv+vec2(-1.,1.)*px));
 let se=luma(sample_color(o.uv+vec2(1.,1.)*px));let m=luma(c);
 let low=min(m,min(min(nw,ne),min(sw,se)));let high=max(m,max(max(nw,ne),max(sw,se)));
 if high-low<max(0.0312,high*0.125) {return vec4(c,1.);}
 var dir=vec2(-((nw+ne)-(sw+se)),(nw+sw)-(ne+se));
 let reduce=max((nw+ne+sw+se)*(0.25*0.125),0.0078125);
 dir=clamp(dir/(min(abs(dir.x),abs(dir.y))+reduce),vec2(-8.),vec2(8.))*px;
 let a=0.5*(sample_color(o.uv+dir*(1./3.-0.5))+sample_color(o.uv+dir*(2./3.-0.5)));
 let b=a*0.5+0.25*(sample_color(o.uv+dir*(-0.5))+sample_color(o.uv+dir*0.5));
 let lb=luma(b);return vec4(select(b,a,lb<low || lb>high),1.);
}
