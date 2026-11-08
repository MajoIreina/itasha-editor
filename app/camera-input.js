/* Register before the archived engine's handlers; access its canvas lazily. */
window.SU7CameraInput=class {
  constructor({element,navigation,active,anchor,onClick,onFocus,onFocusSelected,onShortcut=()=>{}}){
    this.getElement=element;this.getNavigation=navigation;this.active=active;this.anchor=anchor;this.onClick=onClick;this.onFocus=onFocus;this.onFocusSelected=onFocusSelected;this.onShortcut=onShortcut;
    this.listeners=[];this.pointer=null;this.hover=null;
    const listen=(node,type,fn,options=true)=>{node.addEventListener(type,fn,options);this.listeners.push(()=>node.removeEventListener(type,fn,options));};
    const stop=e=>{e.preventDefault();e.stopImmediatePropagation();};
    const inside=e=>this.active()&&e.target===this.element;
    listen(document,'pointerdown',e=>{
      if(!inside(e)||this.pointer||e.isPrimary===false||![0,1,2].includes(e.button))return;
      stop(e);const element=this.element;element.focus({preventScroll:true});element.setPointerCapture(e.pointerId);
      this.pointer={id:e.pointerId,button:e.button,mode:e.button!==0||e.shiftKey?'pan':'orbit',x:e.clientX,y:e.clientY,startX:e.clientX,startY:e.clientY,moved:false,state:this.navigation.state};element.style.cursor=this.pointer.mode==='pan'?'grabbing':'move';
    });
    listen(document,'pointermove',e=>{
      if(inside(e))this.hover={x:e.clientX,y:e.clientY};
      const p=this.pointer;if(!p||p.id!==e.pointerId)return;stop(e);
      if(!this.active()){this.cancel();return;}
      if(!p.moved&&Math.hypot(e.clientX-p.startX,e.clientY-p.startY)<3)return;
      p.moved=true;const dx=e.clientX-p.x,dy=e.clientY-p.y;p.x=e.clientX;p.y=e.clientY;
      this.navigation[p.mode==='pan'?'pan':'orbit'](dx,dy);
    });
    listen(document,'pointerup',e=>{
      const p=this.pointer;if(!p||p.id!==e.pointerId)return;stop(e);this.finish();
      if(!p.moved&&p.button===0&&p.mode==='orbit'&&!e.altKey)this.onClick(e);
    });
    listen(document,'pointercancel',e=>{if(this.pointer?.id===e.pointerId){stop(e);this.cancel();}});
    listen(document,'lostpointercapture',e=>{if(e.target===this.element&&this.pointer)this.cancel();});
    listen(window,'blur',()=>this.cancel());
    listen(document,'wheel',e=>{
      if(!inside(e))return;stop(e);if(this.pointer)return;
      const delta=e.deltaY*(e.deltaMode===1?16:e.deltaMode===2?this.element.clientHeight:1),navigation=this.navigation;
      if(e.altKey)navigation.setFov(navigation.state.fov+Math.max(-160,Math.min(160,delta))*.025);
      else navigation.dolly(delta,this.anchor(e.clientX,e.clientY));
    },{capture:true,passive:false});
    listen(document,'contextmenu',e=>{if(inside(e))stop(e);});
    listen(document,'click',e=>{if(inside(e))stop(e);});
    listen(document,'dblclick',e=>{if(inside(e)&&e.button===0){stop(e);this.onFocus(e.clientX,e.clientY);}});
    listen(document,'keydown',e=>{
      if(!this.active()||e.altKey||/INPUT|TEXTAREA|SELECT/.test(e.target.tagName))return;
      if(e.ctrlKey||e.metaKey){if(['s','z','y','d'].includes(e.key.toLowerCase())){stop(e);this.cancel();this.onShortcut(e);}return;}
      if(e.key==='Escape'&&this.pointer){stop(e);this.cancel();}
      else if(e.key==='Home'){stop(e);this.cancel();this.navigation.fit();}
      else if(e.key.toLowerCase()==='f'){stop(e);if(!this.onFocusSelected()&&this.hover)this.onFocus(this.hover.x,this.hover.y);}
      else if(['Escape','Delete','Backspace','ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key)){stop(e);this.onShortcut(e);}
    });
  }
  get element(){return this.getElement();}
  get navigation(){return this.getNavigation();}
  prepare(){const element=this.element;element.style.touchAction='none';element.tabIndex=0;element.setAttribute('aria-label','3D 车辆视图：左键旋转，右键或中键平移，滚轮推进，Alt 加滚轮调整视场角');}
  finish(){const p=this.pointer;this.pointer=null;const element=this.element;if(!element)return;element.style.cursor='';if(p&&element.hasPointerCapture(p.id))element.releasePointerCapture(p.id);}
  cancel(){if(!this.pointer)return;const state=this.pointer.state;this.finish();this.navigation?.restore(state);}
  dispose(){this.finish();this.listeners.forEach(remove=>remove());this.listeners=[];}
};
