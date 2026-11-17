struct Camera { vp: mat4x4<f32>, eye: vec4<f32>, inverse_vp: mat4x4<f32>, sun:vec4<f32>, environment:vec4<f32>, ground:vec4<f32>, footprint:vec4<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
fn sunlight() -> vec3<f32> {
 return mix(vec3(1.,0.34,0.12),vec3(1.,0.96,0.86),clamp(camera.sun.y*2.,0.,1.));
}
fn sky_color(direction:vec3<f32>) -> vec3<f32> {
 let d=normalize(direction);let day=camera.environment.x;
 let zenith=mix(vec3(0.003,0.006,0.019),vec3(0.13,0.35,0.75),day);
 let horizon=mix(vec3(0.012,0.018,0.045),vec3(0.61,0.76,0.93),day);
 let dusk=(1.-smoothstep(0.,0.35,abs(camera.sun.y)));
 let glow=pow(max(dot(d,camera.sun.xyz),0.),12.);
 var sky=mix(horizon,zenith,pow(max(d.y,0.),0.45));
 sky+=vec3(0.65,0.12,0.025)*dusk*glow;
 sky+=sunlight()*pow(max(dot(d,camera.sun.xyz),0.),900.)*camera.sun.w*2.;
 let ground=mix(vec3(0.006,0.008,0.014),vec3(0.11,0.13,0.16),day);
 return mix(ground,sky,smoothstep(-0.03,0.015,d.y));
}
