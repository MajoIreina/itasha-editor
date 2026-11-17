struct Out { @builtin(position) p:vec4<f32>, @location(0) uv:vec2<f32> }
@vertex fn vs(@builtin(vertex_index) index:u32)->Out {
 let uv=vec2(f32((index<<1u)&2u),f32(index&2u));var o:Out;o.p=vec4(uv*2.-1.,0.,1.);o.uv=uv;return o;
}
@fragment fn fs(o:Out)->@location(0) vec4<f32> {
 let world=camera.inverse_vp*vec4(o.uv*2.-1.,1.,1.);
 let d=normalize(world.xyz/world.w-camera.eye.xyz);
 let distance=(camera.ground.y-camera.eye.y)/min(d.y,-0.0001);
 let point=camera.eye.xyz+d*distance;
 let grid_uv=point.xz-camera.ground.xz;
 let derivatives=max(fwidth(grid_uv),vec2(0.0001));
 if camera.ground.w>0.5 && d.y < -0.01 {
   // Ray-cast the ground plane in world space in the background pass. The
   // vehicle remains the only selectable geometry, while the plane fills the
   // horizon and supplies a soft scale-aware contact shadow below its tires.
   if distance>0. {
     let local=(point.xz-camera.ground.xz)/max(camera.footprint.xy,vec2(0.01));
     let radial=length(local);
     let broad=1.-smoothstep(0.78,1.65,radial);
     let contact=1.-smoothstep(0.42,0.92,radial);
     let day=camera.environment.x;
     let sun_up=max(camera.sun.y,0.);
     var floor=mix(vec3(0.055,0.062,0.072),vec3(0.22,0.245,0.275),day);
     floor*=0.72+0.18*day+0.10*sun_up;
     floor*=1.-broad*0.36-contact*0.30;
     // One-metre floor seams remain subtle near the car and fade at distance;
     // derivative filtering prevents thin lines from aliasing when zoomed out.
     let grid_distance=abs(fract(grid_uv-vec2(0.5))-vec2(0.5))/derivatives;
     let grid=1.-clamp(min(grid_distance.x,grid_distance.y),0.,1.);
     let grid_fade=(1.-smoothstep(8.,22.,distance))*smoothstep(0.015,0.08,-d.y);
     floor*=1.-grid*0.12*grid_fade;
     let haze=select(0.,1.-exp(-distance*0.006),camera.environment.y>0.5);
     floor=mix(floor,vec3(0.40,0.49,0.60)*mix(0.2,1.,day),haze*0.52);
     return vec4(floor,1.);
   }
 }
 if camera.environment.y<0.5 { return vec4(vec3(mix(0.032,0.88,camera.environment.z)),1.); }
 return vec4(sky_color(d),1.);
}
