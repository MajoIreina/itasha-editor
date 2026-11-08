/* Preview illumination; source pixels and factory paint colors stay intact. */
window.SU7Lighting=class {
  constructor(runtime){this.runtime=runtime;this.lights=new Map();this.materials=new WeakMap();this.floors=new WeakMap();this.originalBackground=runtime.scene.background;this.tint=new runtime.Vector3(1,1,1);this.backgroundTint=new runtime.Vector3(1,1,1);this.value={mode:'studio',hour:12,brightness:1,temperature:6500};const previous=runtime.scene.onBeforeRender;runtime.scene.onBeforeRender=(...args)=>{previous?.apply(runtime.scene,args);this.applyBackground();};}
  static temperature(k){const t=k/100,clamp=x=>Math.max(0,Math.min(255,x))/255;return [clamp(t<=66?255:329.698727446*Math.pow(t-60,-.1332047592)),clamp(t<=66?99.4708025861*Math.log(t)-161.1195681661:288.1221695283*Math.pow(t-60,-.0755148492)),clamp(t>=66?255:t<=19?0:138.5177312231*Math.log(t-10)-305.0447927307)];}
  set(value){this.value={...this.value,...value};this.apply();}
  base(material){return this.materials.get(material);}
  applyBackground(){const v=this.value,r=this.runtime;if(v.mode==='white')r.scene.background=this.whiteColor;else if(v.mode==='time')r.scene.background=this.skyTexture;}
  updateSky(day,twilight,tint){
    const r=this.runtime,v=this.value,key=[v.mode,v.hour,v.temperature,v.brightness].join();if(key===this.skyKey||v.mode!=='time')return;this.skyKey=key;
    if(!this.skyTexture){this.skyCanvas=document.createElement('canvas');this.skyCanvas.width=4;this.skyCanvas.height=256;this.skyTexture=new r.Texture(this.skyCanvas);this.skyTexture.colorSpace='srgb';}
    const x=this.skyCanvas.getContext('2d'),g=x.createLinearGradient(0,0,0,256),blend=(night,noon,sunset)=>night.map((n,i)=>(n*(1-day)+noon[i]*day)*(1-.65*twilight)+sunset.map((n,i)=>n*.65*twilight)[i]),color=rgb=>'rgb('+rgb.map((n,i)=>Math.round(Math.min(255,Math.max(0,n*Math.sqrt(tint[i])*Math.sqrt(v.brightness))))).join(',')+')';
    g.addColorStop(0,color(blend([7,11,22],[92,156,207],[55,47,77])));g.addColorStop(.6,color(blend([24,34,57],[210,233,244],[238,159,106])));g.addColorStop(1,color(blend([16,21,32],[155,177,189],[121,100,92])));x.fillStyle=g;x.fillRect(0,0,4,256);this.skyTexture.needsUpdate=true;
  }
  apply(){
    const {runtime:r,value:v}=this,time=v.mode==='time',h=v.hour,elevation=Math.sin((h-6)*Math.PI/12),day=Math.max(0,elevation),twilight=Math.exp(-Math.pow((Math.abs(h-12)-6)/1.5,2)),ambient=time?.25+.75*Math.sqrt(day)+.1*twilight:1,key=time?.08+1.07*day+.4*twilight:1,env=time?.24+.76*Math.sqrt(day)+.12*twilight:1;
    const temp=SU7Lighting.temperature(v.temperature),neutral=SU7Lighting.temperature(6500),tint=temp.map((x,i)=>Math.pow(x/neutral[i],2.2));
    this.tint.set(...tint.map(Math.sqrt));this.backgroundTint.set(...tint.map(x=>Math.sqrt(x)*v.brightness*env));this.updateSky(day,twilight,tint);
    const white=v.mode==='white';if(!this.whiteColor){const Color=r.scene.getObjectByName('carPaint_external_4').material.color.constructor;this.whiteColor=new Color().setRGB(1000,1000,1000);}r.scene.background=white?this.whiteColor:time?this.skyTexture:this.originalBackground;
    r.scene.traverse(o=>{
      if(o.isMesh&&(/^floor_[12]$/.test(o.name)||o.name==='shadow')){if(!this.floors.has(o))this.floors.set(o,{visible:o.visible,color:o.material.color?.clone(),lightColor:o.material.uniforms?.lightColor?.value.clone(),lightIntensity:o.material.uniforms?.lightIntensity?.value});if(o.name==='floor_2'&&!o.material.userData.studioBackgroundTint){const m=o.material,original=m.onBeforeCompile,key=m.customProgramCacheKey(),uniform={value:this.backgroundTint};m.onBeforeCompile=function(shader,...args){original?.call(this,shader,...args);shader.uniforms.studioBackgroundTint=uniform;shader.fragmentShader='uniform vec3 studioBackgroundTint;\n'+shader.fragmentShader;shader.fragmentShader=shader.fragmentShader.replace('#include <tonemapping_fragment>','gl_FragColor.rgb *= studioBackgroundTint;\n#include <tonemapping_fragment>');};m.customProgramCacheKey=()=>key+'-studio-sky-v1';m.userData.studioBackgroundTint=true;m.needsUpdate=true;}const b=this.floors.get(o);o.visible=white?false:b.visible;if(b.color){o.material.color.copy(b.color).multiplyScalar(v.brightness*env);o.material.color.r*=tint[0];o.material.color.g*=tint[1];o.material.color.b*=tint[2];}if(b.lightColor){const u=o.material.uniforms;u.lightColor.value.copy(b.lightColor);u.lightColor.value.r*=tint[0];u.lightColor.value.g*=tint[1];u.lightColor.value.b*=tint[2];u.lightIntensity.value=b.lightIntensity*v.brightness*env;}}
      if(o.isLight){
        if(!this.lights.has(o))this.lights.set(o,{intensity:o.intensity,color:o.color.clone(),position:o.position.clone()});
        const b=this.lights.get(o);o.intensity=b.intensity*v.brightness*(o.isAmbientLight?ambient:key);o.color.copy(b.color);o.color.r*=tint[0];o.color.g*=tint[1];o.color.b*=tint[2];
        o.position.copy(b.position);
        if(time&&o.isDirectionalLight){const az=(h-12)*Math.PI/12,rad=Math.max(5,Math.hypot(b.position.x,b.position.z)),offset=Math.atan2(b.position.z,b.position.x);o.position.set(Math.sin(az+offset)*rad,Math.max(.5,day*rad),Math.cos(az+offset)*rad);}
      }
      if(o.isMesh)for(const m of Array.isArray(o.material)?o.material:[o.material]){
        if(!m||!('envMapIntensity' in m))continue;
        if(!this.materials.has(m)){this.materials.set(m,{envMapIntensity:m.envMapIntensity,lightMapIntensity:m.lightMapIntensity});if(m.isMeshPhysicalMaterial){const original=m.onBeforeCompile,key=m.customProgramCacheKey(),uniform={value:this.tint};m.onBeforeCompile=function(shader,...args){original?.call(this,shader,...args);shader.uniforms.studioIlluminationTint=uniform;shader.fragmentShader='uniform vec3 studioIlluminationTint;\n'+shader.fragmentShader;shader.fragmentShader=shader.fragmentShader.replace('#include <tonemapping_fragment>','gl_FragColor.rgb *= studioIlluminationTint;\n#include <tonemapping_fragment>');};m.customProgramCacheKey=()=>key+'-studio-illumination-v1';m.needsUpdate=true;}}
        const b=this.materials.get(m);m.envMapIntensity=b.envMapIntensity*v.brightness*env;if(m.lightMap)m.lightMapIntensity=b.lightMapIntensity*v.brightness*env;
      }
    });
    this.actual={...v,daylight:day,ambientFactor:ambient,keyFactor:key,environmentFactor:env,lights:this.lights.size};
  }
};
