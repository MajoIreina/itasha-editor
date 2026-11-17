import { useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { artworkBitmap, drawCoverage, coveragePoint, imageGrabOffset, type CoverageFragment } from './canvasEngine';
import type { CanvasEditor } from './useCanvasEditor';
import type { Action, Layer, Project } from '../../bridge/desktop';
import { mutation } from '../../bridge/desktop';
import { chartVector, controlPoints, coverageMetricFrame, IDENTITY_FRAME, nearestControl, transformMetricLayer, type MetricFrame } from './metricControls';
import { wholeLayout } from './wholeLayout';
import './canvas.css';
type Point={x:number;y:number;surfaceId:string};
export function DesignCanvas({project,editor,busy,suspended,act}:{project:Project|null;editor:CanvasEditor;busy:boolean;suspended:boolean;act:(action:Action)=>Promise<boolean>}) {
 const canvas=useRef<HTMLCanvasElement>(null),container=useRef<HTMLDivElement>(null);
 const [size,setSize]=useState({width:1,height:1}),[zoom,setZoom]=useState(1),[pan,setPan]=useState({x:0,y:0});
 const [mode,setMode]=useState<'whole'|'part'>('whole');
 const [preview,setPreview]=useState<Layer|null>(null);
 const panGesture=useRef<{x:number;y:number;pan:{x:number;y:number}}|null>(null);
 const [coverage,setCoverage]=useState(new Map<string,{key:string;surfaceId:string;fragments:CoverageFragment[];frame:MetricFrame|null}>());
 const settling=useRef(false);
 const gesture=useRef<{pending:Promise<void>;sequence:number;cancelled:boolean;original:Layer;base:Layer;value:Layer;start:Point;grabOffset:{x:number;y:number};frame:MetricFrame|null;handle:number|null;rotate:boolean;mutation:ReturnType<typeof mutation>}|null>(null);
 const layout=useMemo(()=>wholeLayout(editor.surfaces),[editor.surfaces]);
 const tiles=useMemo(()=>mode==='whole'?layout.tiles:editor.surface?[{surface:editor.surface,x:0,y:0}]:[],[mode,layout,editor.surface]);
 const width=Math.max(1,...tiles.map(t=>t.x+t.surface.widthMm)),height=Math.max(1,...tiles.map(t=>t.y+t.surface.heightMm));
 const scale=Math.min(Math.max(1,size.width-56)/width,Math.max(1,size.height-56)/height)*zoom;
 const origin={x:(size.width-width*scale)/2+pan.x,y:(size.height-height*scale)/2+pan.y};
 const layers=useMemo(()=>project?.editor.layers.map(l=>preview?.id===l.id?preview:editor.preview?.id===l.id?editor.preview:l)??[],[project?.editor.layers,preview,editor.preview]);
 const bitmaps=useMemo(()=>new Map(layers.map(l=>[l.id,artworkBitmap(l,editor.images)])),[layers.map(l=>JSON.stringify(l.source)).join('|'),editor.images]);
 useEffect(()=>{const el=container.current;if(!el)return;const observer=new ResizeObserver(()=>setSize({width:el.clientWidth,height:el.clientHeight}));observer.observe(el);return()=>observer.disconnect();},[]);
 useEffect(()=>{setZoom(1);setPan({x:0,y:0});},[mode,mode==='part'?editor.surfaceId:'whole']);
 useEffect(()=>{if(gesture.current)gesture.current.cancelled=true;gesture.current=null;setPreview(null);},[project?.revision,project?.session.sessionEpoch,suspended]);
 useEffect(()=>{
  if(!project||!editor.writable)return;let disposed=false;
  void Promise.all(layers.map(async layer=>{
   const key=JSON.stringify(layer.placement),cached=coverage.get(layer.id);
   if(cached?.key===key)return [layer.id,cached] as const;
   const fragments=await invoke<CoverageFragment[]>('viewport_coverage',{session:project.session,layer});
   return [layer.id,{key,surfaceId:layer.placement.surfaceId,fragments,frame:coverageMetricFrame(layer,fragments)}] as const;
  })).then(entries=>{if(!disposed)setCoverage(new Map(entries));}).catch(()=>{});
  return()=>{disposed=true;};
 },[layers,project?.session.sessionEpoch,editor.writable]);
 const locate=(clientX:number,clientY:number):Point|null=>{
  const rect=canvas.current!.getBoundingClientRect(),x=(clientX-rect.x-origin.x)/scale,y=(clientY-rect.y-origin.y)/scale;
  const ctx=document.createElement('canvas').getContext('2d')!;
  const tile=tiles.find(t=>ctx.isPointInPath(t.surface.mask,x-t.x,y-t.y));
  return tile?{x:x-tile.x,y:y-tile.y,surfaceId:tile.surface.id}:null;
 };
 useEffect(()=>{
  const element=canvas.current;if(!element)return;
  return editor.registerDropTarget('canvas',{element,locate:(x,y)=>{
   if(suspended||busy||!editor.writable)return null;const p=locate(x,y);
   return p?{surfaceId:p.surfaceId,xMm:p.x,yMm:p.y,cross:editor.crossMode}:null;
  }});
 },[tiles,size,zoom,pan,suspended,busy,editor.writable,editor.crossMode]);
 useEffect(()=>{
  const frame=requestAnimationFrame(()=>{
   const el=canvas.current;if(!el)return;const ratio=window.devicePixelRatio;
   el.width=Math.max(1,Math.round(size.width*ratio));el.height=Math.max(1,Math.round(size.height*ratio));
   const ctx=el.getContext('2d')!;ctx.scale(ratio,ratio);
   const light=document.documentElement.dataset.theme==='light';ctx.fillStyle=light?'#e9e9e9':'#181818';ctx.fillRect(0,0,size.width,size.height);
   ctx.translate(origin.x,origin.y);ctx.scale(scale,scale);
   if(mode==='whole'&&tiles.length){
    const r=layout.reference;
    ctx.save();ctx.strokeStyle=light?'#a8afb7':'#515b66';ctx.lineWidth=1/scale;ctx.setLineDash([5/scale,5/scale]);
    ctx.strokeRect(r.x,r.y,r.width,r.height);ctx.setLineDash([]);
    ctx.fillStyle=light?'#59636e':'#a7b2c0';ctx.font=`${11/scale}px "Segoe UI"`;ctx.textAlign='center';
    ctx.fillText('车头 ↑',r.x+r.width/2,r.y+24/scale);
    ctx.fillText('车顶 / 玻璃 · 仅定位，不可绘制',r.x+r.width/2,r.y+r.height/2);
    ctx.restore();
   }
   for(const tile of tiles){
    ctx.save();ctx.translate(tile.x,tile.y);ctx.fillStyle=light?'#d4dce4':'#3b424a';ctx.fill(tile.surface.mask);
    for(const layer of layers){const data=coverage.get(layer.id),bitmap=bitmaps.get(layer.id);if(data&&bitmap)drawCoverage(ctx,layer,bitmap,data.fragments,tile.surface.id);}
    const selected=layers.find(l=>l.id===editor.selectedId);
    if(selected?.placement.surfaceId===tile.surface.id&&selected.visible){
     const data=coverage.get(selected.id),active=gesture.current;
     const frame=active?.base.id===selected.id&&active.frame?active.frame:data?.surfaceId===selected.placement.surfaceId&&data.frame?data.frame:IDENTITY_FRAME;
     const points=controlPoints(selected,frame,22/scale);
     ctx.strokeStyle='#60a5fa';ctx.lineWidth=1.5/scale;ctx.beginPath();
     for(const [order,index]of [0,2,4,6].entries()){const point=points[index];if(order)ctx.lineTo(point.x,point.y);else ctx.moveTo(point.x,point.y);}ctx.closePath();ctx.stroke();
     if(!selected.locked){ctx.fillStyle=light?'#fff':'#202020';for(const point of points.slice(0,8)){ctx.fillRect(point.x-4/scale,point.y-4/scale,8/scale,8/scale);ctx.strokeRect(point.x-4/scale,point.y-4/scale,8/scale,8/scale);}
      ctx.beginPath();ctx.moveTo(points[1].x,points[1].y);ctx.lineTo(points[8].x,points[8].y);ctx.stroke();ctx.beginPath();ctx.arc(points[8].x,points[8].y,5/scale,0,Math.PI*2);ctx.fill();ctx.stroke();}
    }
    ctx.restore();
   }
   ctx.resetTransform();
   ctx.scale(ratio,ratio);
   // Labels use screen pixels so small parts remain identifiable at fit-to-view.
   // Keep them disjoint; leader lines retain the association when space is tight.
   ctx.font='11px "Segoe UI"';
   const labels:{x:number;y:number;w:number;h:number}[]=[];
   for(const tile of tiles){
    const anchor={x:origin.x+(tile.x+tile.surface.widthMm/2)*scale,y:origin.y+tile.y*scale};
    const w=ctx.measureText(tile.surface.name).width+10,h=18;
    const x=Math.max(4,Math.min(size.width-w-4,anchor.x-w/2));
    let y=Math.max(4,anchor.y-h-4);
    while(labels.some(r=>x<r.x+r.w+3&&x+w+3>r.x&&y<r.y+r.h+3&&y+h+3>r.y)&&y>h+4)y-=h+3;
    labels.push({x,y,w,h});
    if(Math.abs(y+h-anchor.y)>8){ctx.beginPath();ctx.moveTo(anchor.x,anchor.y);ctx.lineTo(x+w/2,y+h);ctx.strokeStyle=light?'#8e969f':'#687787';ctx.lineWidth=.75;ctx.stroke();}
    ctx.fillStyle=light?'#e9e9e9ee':'#181818ee';ctx.fillRect(x,y,w,h);
    ctx.fillStyle=tile.surface.id===editor.surfaceId?'#60a5fa':light?'#252525':'#f1f1f1';
    ctx.fillText(tile.surface.name,x+5,y+12);
   }
   ctx.resetTransform();
  });return()=>cancelAnimationFrame(frame);
 },[tiles,size,zoom,pan,layers,coverage,bitmaps,editor.selectedId,editor.surfaceId]);
 const cancel=()=>{panGesture.current=null;if(gesture.current)gesture.current.cancelled=true;gesture.current=null;setPreview(null);void editor.cancelGesture();};
 return <section className="canvas design-canvas"><div className="panel-title"><span>2D 漆面展开</span><div className="canvas-controls">
  <button onClick={()=>setMode(m=>m==='whole'?'part':'whole')}>{mode==='whole'?'单部件':'整车展开'}</button>
  <select aria-label="编辑部件" value={editor.surfaceId} onChange={e=>{editor.setSurfaceId(e.target.value);setMode('part');}}>{editor.surfaces.map(s=><option key={s.id} value={s.id}>{s.name}</option>)}</select>
  <label title="素材沿已连接的相邻漆面连续显示；关闭后仅显示在当前部件"><input type="checkbox" checked={editor.crossMode} disabled={busy||Boolean(editor.selected?.locked)} onChange={e=>void editor.toggleCross(e.target.checked)}/>跨相邻漆面</label>
  <button onClick={()=>{setZoom(1);setPan({x:0,y:0});}}>适应</button><span>{Math.round(zoom*100)}%</span>
 </div></div><div className="canvas-workarea" ref={container}><canvas ref={canvas} tabIndex={0} aria-label="车身贴花画布" onContextMenu={e=>e.preventDefault()}
 onWheel={e=>{if(!gesture.current)setZoom(z=>Math.max(.25,Math.min(16,z*Math.exp(-e.deltaY*.001))));}}
 onDoubleClick={e=>{const p=locate(e.clientX,e.clientY);if(p){editor.setSurfaceId(p.surfaceId);setMode('part');}}}
 onPointerDown={e=>{
  if(e.button===1||(e.button===0&&e.altKey)){e.preventDefault();panGesture.current={x:e.clientX,y:e.clientY,pan};e.currentTarget.setPointerCapture(e.pointerId);return;}
  if(!project||busy||suspended||settling.current||!editor.writable||e.button!==0)return;e.currentTarget.focus();
  const r=e.currentTarget.getBoundingClientRect(),gx=(e.clientX-r.x-origin.x)/scale,gy=(e.clientY-r.y-origin.y)/scale;
  let layer=layers.find(l=>l.id===editor.selectedId)??null,handle:number|null=null,rotate=false,p=locate(e.clientX,e.clientY),grabOffset:{x:number;y:number}|null=null,metricFrame:MetricFrame|null=null;
  if(layer&&!layer.locked){const tile=tiles.find(t=>t.surface.id===layer!.placement.surfaceId);if(tile){
   const data=coverage.get(layer.id);
   metricFrame=data?.key===JSON.stringify(layer.placement)?data.frame:null;
   const index=metricFrame?nearestControl(controlPoints(layer,metricFrame,22/scale),{x:gx-tile.x,y:gy-tile.y},12/scale):null;
   if(index!==null&&index<8)handle=index;rotate=index===8;
   if(handle!==null||rotate)p={x:gx-tile.x,y:gy-tile.y,surfaceId:tile.surface.id};
  }}
  if(handle===null&&!rotate){layer=p?[...layers].reverse().find(l=>{const f=coverage.get(l.id),b=bitmaps.get(l.id),point=f&&b?coveragePoint(l,b,f.fragments,p!.surfaceId,p!.x,p!.y):null;if(point)grabOffset=imageGrabOffset(l,point);return point!==null;})??null:null;editor.setSelectedId(layer?.id??null);}
  if(!layer||layer.locked||!p)return;
  editor.setSurfaceId(layer.placement.surfaceId);e.currentTarget.setPointerCapture(e.pointerId);
  gesture.current={pending:Promise.resolve(),sequence:0,cancelled:false,original:layer,base:layer,value:layer,start:p,grabOffset:grabOffset??{x:0,y:0},frame:metricFrame,handle,rotate,mutation:mutation(project)};
  void editor.beginGesture(layer);
 }}
 onPointerMove={e=>{
  const pg=panGesture.current;if(pg){setPan({x:pg.pan.x+e.clientX-pg.x,y:pg.pan.y+e.clientY-pg.y});return;}
  const g=gesture.current;if(!g)return;let p=locate(e.clientX,e.clientY);
  if(g.handle!==null||g.rotate){const tile=tiles.find(t=>t.surface.id===g.base.placement.surfaceId),r=e.currentTarget.getBoundingClientRect();if(tile)p={surfaceId:tile.surface.id,x:(e.clientX-r.x-origin.x)/scale-tile.x,y:(e.clientY-r.y-origin.y)/scale-tile.y};}
  if(!p)return;
  if(g.handle===null&&!g.rotate&&p.surfaceId===g.start.surfaceId&&Math.hypot(p.x-g.start.x,p.y-g.start.y)<1e-6)return;
  if(g.handle===null&&!g.rotate){
   const point=p,sequence=++g.sequence;
   g.pending=g.pending.then(async()=>{
    if(g.cancelled||sequence!==g.sequence)return;
    try {
     const value=await invoke<Layer>('viewport_transport',{session:project!.session,layer:g.original,surfaceId:point.surfaceId,point:[point.x,point.y],offset:[g.grabOffset.x,g.grabOffset.y]});
     if(g.cancelled||sequence!==g.sequence)return;
     g.value=value;setPreview(value);await editor.previewGesture(value);
    } catch { /* Keep the last valid placement when the grabbed centre leaves paint. */ }
   });return;
  }
  else {
   if(p.surfaceId!==g.base.placement.surfaceId||!g.frame)return;
   const value=transformMetricLayer(g.base,g.frame,g.start,p,g.handle,g.rotate,e.shiftKey),surface=editor.surfaces.find(s=>s.id===g.base.placement.surfaceId);
   if(!value||!surface||!canvas.current?.getContext('2d')?.isPointInPath(surface.mask,value.placement.xMm,value.placement.yMm))return;
   g.value=value;
  }
  setPreview(g.value);void editor.previewGesture(g.value);
 }}
 onPointerUp={e=>{if(panGesture.current){panGesture.current=null;e.currentTarget.releasePointerCapture(e.pointerId);return;}const g=gesture.current;if(!g)return;gesture.current=null;e.currentTarget.releasePointerCapture(e.pointerId);settling.current=true;void g.pending.then(()=>g.cancelled?undefined:editor.commitGesture()).finally(()=>{settling.current=false;setPreview(null);});}}
 onPointerCancel={cancel} onLostPointerCapture={()=>{if(gesture.current||panGesture.current)cancel();}} onBlur={()=>{if(gesture.current)cancel();}}
 onKeyDown={e=>{if(e.key==='Escape'){cancel();return;}const layer=editor.selected;if(!project||!layer||layer.locked||busy||suspended||gesture.current)return;
  if(e.key==='Delete'||e.key==='Backspace'){e.preventDefault();void editor.edit({type:'delete',id:layer.id});}
  else if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key)){
   e.preventDefault();const data=coverage.get(layer.id),frame=data?.key===JSON.stringify(layer.placement)?data.frame:null;
   if(!frame)return;const d=e.shiftKey?10:1,delta=chartVector(frame,{x:e.key==='ArrowLeft'?-d:e.key==='ArrowRight'?d:0,y:e.key==='ArrowUp'?-d:e.key==='ArrowDown'?d:0});
   const value={...layer,placement:{...layer.placement,xMm:layer.placement.xMm+delta.x,yMm:layer.placement.yMm+delta.y}},surface=editor.surfaces.find(s=>s.id===layer.placement.surfaceId);
   if(surface&&canvas.current?.getContext('2d')?.isPointInPath(surface.mask,value.placement.xMm,value.placement.yMm))void act({type:'edit',mutation:mutation(project),operation:{type:'update',layer:value}});
  }
 }}/>{!tiles.length&&<div className="canvas-loading">{editor.message}</div>}</div><div className="canvas-status"><span>{mode==='whole'?'整车展开 · 车头朝上':editor.surface?.name} · 展开候选资产</span><span title={editor.message}>{editor.message}</span></div></section>;
}
