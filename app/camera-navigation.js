/* Camera state and projection math. No renderer, DOM listeners or animation loop. */
(function(root){
  'use strict';
  const clamp=(v,a,b)=>Math.max(a,Math.min(b,v)),rad=Math.PI/180;
  const add=(a,b,s=1)=>a.map((v,i)=>v+b[i]*s),dot=(a,b)=>a.reduce((n,v,i)=>n+v*b[i],0);
  const limits=Object.freeze({fov:[15,90],distance:[.12,100],pitch:[-10,90]});
  const vehicle={min:[-2.6,0,-1.1],max:[2.6,1.6,1.1]};
  const views={hero:[-44,9],left:[0,0],right:[180,0],front:[-90,0],rear:[90,0],top:[0,90]};
  class CameraNavigation {
    constructor({onChange=()=>{},width=1,height=1}={}){
      this.onChange=onChange;this.width=Math.max(1,width);this.height=Math.max(1,height);
      this.framing={min:[...vehicle.min],max:[...vehicle.max]};
      this.value={yaw:-44,pitch:9,fov:30,distance:10,target:[0,.8,0],preserveFraming:true,view:'hero'};
      this.value.distance=this.fitDistance(vehicle,this.value);this.limits=limits;
    }
    get state(){return {...this.value,target:[...this.value.target],framing:{min:[...this.framing.min],max:[...this.framing.max]}};}
    changed(){this.onChange(this.state);return this.state;}
    viewport(width,height){if(Number.isFinite(width)&&Number.isFinite(height)){this.width=Math.max(1,width);this.height=Math.max(1,height);}}
    axes(state=this.value){
      const y=state.yaw*rad,p=state.pitch*rad,sy=Math.sin(y),cy=Math.cos(y),sp=Math.sin(p),cp=Math.cos(p);
      return {direction:[sy*cp,sp,cy*cp],right:[cy,0,-sy],up:[-sy*sp,cp,-cy*sp]};
    }
    pose(state=this.value){const axes=this.axes(state);return {...axes,position:add(state.target,axes.direction,state.distance),target:[...state.target],fov:state.fov,distance:state.distance};}
    fitDistance(bounds,state=this.value,width=this.width,height=this.height){
      const {direction,right,up}=this.axes(state),tanV=Math.tan(state.fov*rad/2),tanH=tanV*Math.max(1,width)/Math.max(1,height);let distance=0;
      for(const x of [bounds.min[0],bounds.max[0]])for(const y of [bounds.min[1],bounds.max[1]])for(const z of [bounds.min[2],bounds.max[2]]){
        const p=add([x,y,z],state.target,-1);distance=Math.max(distance,dot(p,direction)+Math.abs(dot(p,right))/tanH,dot(p,direction)+Math.abs(dot(p,up))/tanV);
      }
      return clamp(distance*1.13,...limits.distance);
    }
    restore(state){
      if(!state||!['yaw','pitch','fov','distance'].every(k=>Number.isFinite(state[k]))||!Array.isArray(state.target)||state.target.length!==3||!state.target.every(Number.isFinite))return false;
      if(state.framing&&['min','max'].every(k=>Array.isArray(state.framing[k])&&state.framing[k].length===3&&state.framing[k].every(Number.isFinite)))this.framing={min:[...state.framing.min],max:[...state.framing.max]};
      this.value={yaw:((state.yaw+180)%360+360)%360-180,pitch:clamp(state.pitch,...limits.pitch),fov:clamp(state.fov,...limits.fov),distance:clamp(state.distance,...limits.distance),target:[...state.target],preserveFraming:state.preserveFraming!==false,view:state.view||'custom'};return this.changed();
    }
    view(name){if(!views[name])return false;[this.value.yaw,this.value.pitch]=views[name];this.value.view=name;return this.changed();}
    orbit(dx,dy){
      if(![dx,dy].every(Number.isFinite))return;
      const speed=180/Math.max(300,this.height);this.value.yaw=((this.value.yaw-dx*speed+180)%360+360)%360-180;this.value.pitch=clamp(this.value.pitch+dy*speed,...limits.pitch);this.value.view='custom';return this.changed();
    }
    pan(dx,dy){
      if(![dx,dy].every(Number.isFinite))return;
      const {right,up}=this.axes(),scale=2*this.value.distance*Math.tan(this.value.fov*rad/2)/this.height;
      this.value.target=add(add(this.value.target,right,-dx*scale),up,dy*scale);return this.changed();
    }
    dolly(delta,anchor){
      if(!Number.isFinite(delta)||!delta)return;
      const old=this.value.distance,next=clamp(old*Math.exp(clamp(delta*.0015,-.6,.6)),...limits.distance),ratio=next/old;
      if(Array.isArray(anchor)&&anchor.length===3&&anchor.every(Number.isFinite))this.value.target=add(anchor,add(this.value.target,anchor,-1),ratio);
      this.value.distance=next;return this.changed();
    }
    setFov(value,preserve=this.value.preserveFraming){
      if(!Number.isFinite(value))return false;
      let fov=clamp(value,...limits.fov);
      if(preserve){
        const scale=this.framingSize(),tan=Math.tan(fov*rad/2);
        if(Number.isFinite(scale)&&scale>1e-8){
          // Preserve the projected bounds of the car or focused artwork, rather
          // than the center plane, which changes size noticeably with a deep body.
          let low=limits.distance[0],high=limits.distance[1];
          for(let i=0;i<44;i++){const distance=(low+high)/2;if(this.framingSize({...this.value,fov,distance})>scale)low=distance;else high=distance;}
          this.value.distance=(low+high)/2;const actual=this.framingSize({...this.value,fov});
          if(Number.isFinite(actual))fov=2*Math.atan(tan*actual/scale)/rad;
        }else{
          // Once the camera enters those bounds, hold the local focus plane.
          const height=this.value.distance*Math.tan(this.value.fov*rad/2),distance=clamp(height/tan,...limits.distance);
          fov=2*Math.atan(height/distance)/rad;this.value.distance=distance;
        }
      }
      this.value.fov=clamp(fov,...limits.fov);return this.changed();
    }
    setFraming(value){this.value.preserveFraming=!!value;return this.changed();}
    framingSize(state=this.value){
      const {direction,right,up}=this.axes(state),xs=[],ys=[],aspect=this.width/this.height,tan=Math.tan(state.fov*rad/2);
      for(const x of [this.framing.min[0],this.framing.max[0]])for(const y of [this.framing.min[1],this.framing.max[1]])for(const z of [this.framing.min[2],this.framing.max[2]]){
        const p=add([x,y,z],state.target,-1),depth=state.distance-dot(p,direction);if(depth<=.005)return Infinity;
        xs.push(dot(p,right)/depth/tan/aspect);ys.push(dot(p,up)/depth/tan);
      }
      return Math.max(Math.max(...xs)-Math.min(...xs),Math.max(...ys)-Math.min(...ys));
    }
    fit(bounds=vehicle){
      this.framing={min:[...bounds.min],max:[...bounds.max]};this.value.target=bounds.min.map((v,i)=>(v+bounds.max[i])/2);this.value.distance=this.fitDistance(bounds);return this.changed();
    }
    focus(point,radius=.5){
      if(!Array.isArray(point)||point.length!==3||!point.every(Number.isFinite))return false;
      const size=Math.max(.08,Math.min(3,radius));return this.fit({min:point.map(v=>v-size),max:point.map(v=>v+size)});
    }
    reset(){const preserve=this.value.preserveFraming;this.value={yaw:-44,pitch:9,fov:30,distance:10,target:[0,.8,0],preserveFraming:preserve,view:'hero'};return this.fit();}
    renderState({yaw,pitch,fov=this.value.fov,width,height,current=false}){
      if(current)return this.state;
      const state={...this.state,yaw,pitch,fov:clamp(fov,...limits.fov),target:[0,.8,0],view:'custom'};state.distance=this.fitDistance(vehicle,state,width,height);return state;
    }
    apply(camera,width=this.width,height=this.height,state=this.value){
      const pose=this.pose(state);camera.clearViewOffset();camera.zoom=1;camera.filmOffset=0;camera.aspect=Math.max(1,width)/Math.max(1,height);camera.fov=pose.fov;
      camera.near=Math.max(.005,Math.min(.05,state.distance/100));camera.far=20000;camera.position.set(...pose.position);camera.up.set(...pose.up);camera.lookAt(...pose.target);camera.updateProjectionMatrix();camera.updateMatrixWorld(true);return camera;
    }
  }
  root.SU7CameraNavigation=CameraNavigation;
  if(typeof module!=='undefined'&&module.exports)module.exports=CameraNavigation;
})(typeof window==='undefined'?globalThis:window);
