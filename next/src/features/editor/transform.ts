
import type { Layer } from '../../bridge/desktop';
export const HANDLES=[[-1,-1],[0,-1],[1,-1],[1,0],[1,1],[0,1],[-1,1],[-1,0]] as const;
export function transformLayer(layer:Layer,start:{x:number;y:number},current:{x:number;y:number},handle:number|null,rotate:boolean,shift:boolean):Layer {
 const p=layer.placement,next={...p},a=p.rotationDeg*Math.PI/180,c=Math.cos(a),s=Math.sin(a);
 const dx=current.x-start.x,dy=current.y-start.y;
 if (rotate) {
  const angle=(v:typeof start)=>Math.atan2(v.y-p.yMm,v.x-p.xMm);
  next.rotationDeg=((p.rotationDeg+(angle(current)-angle(start))*180/Math.PI+540)%360)-180;
  if(shift)next.rotationDeg=Math.round(next.rotationDeg/15)*15;
 } else if(handle!==null) {
  const [hx,hy]=HANDLES[handle],x=dx*c+dy*s,y=-dx*s+dy*c;
  next.widthMm=hx?Math.max(1,Math.min(10000,p.widthMm+x*hx)):p.widthMm;
  next.heightMm=hy?Math.max(1,Math.min(10000,p.heightMm+y*hy)):p.heightMm;
  if(p.aspectRatioLocked||(shift&&hx&&hy)){
   const sx=1+x*hx/p.widthMm,sy=1+y*hy/p.heightMm;
   const factor=!hx?sy:!hy||!p.aspectRatioLocked||Math.abs(sx-1)>=Math.abs(sy-1)?sx:sy;
   const scale=Math.max(Math.max(1/p.widthMm,1/p.heightMm),Math.min(Math.min(10000/p.widthMm,10000/p.heightMm),factor));
   next.widthMm=p.widthMm*scale;next.heightMm=p.heightMm*scale;
  }
  const ox=(next.widthMm-p.widthMm)*hx/2,oy=(next.heightMm-p.heightMm)*hy/2;
  next.xMm+=ox*c-oy*s;next.yMm+=ox*s+oy*c;
 } else {next.xMm+=dx;next.yMm+=dy;}
 return {...layer,placement:next};
}

/** Re-anchor a layer to another real paint chart while keeping the grabbed point under the pointer. */
export function moveLayerAnchor(layer:Layer,surfaceId:string,point:{x:number;y:number},grabOffset:{x:number;y:number}):Layer {
 const p=layer.placement,a=p.rotationDeg*Math.PI/180,c=Math.cos(a),s=Math.sin(a);
 return {...layer,placement:{...p,surfaceId,xMm:point.x-(grabOffset.x*c-grabOffset.y*s),yMm:point.y-(grabOffset.x*s+grabOffset.y*c)}};
}

// Sample the rotated perimeter against the actual panel mask, including curved edges.
export function footprint(layer:Layer):{x:number;y:number}[]{
 const p=layer.placement,a=p.rotationDeg*Math.PI/180,c=Math.cos(a),s=Math.sin(a),points=[];
 for(let i=0;i<=8;i++)for(const [x,y] of [[i/8-.5,-.5],[i/8-.5,.5],[-.5,i/8-.5],[.5,i/8-.5]])
  points.push({x:p.xMm+x*p.widthMm*c-y*p.heightMm*s,y:p.yMm+x*p.widthMm*s+y*p.heightMm*c});
 return points;
}
export function constrainToPanel(previous:Layer,next:Layer,contains:(x:number,y:number)=>boolean):Layer {
 if(next.placement.crossSurface)return next;
 if(previous.placement.surfaceId===next.placement.surfaceId&&Boolean(previous.placement.crossSurface)===Boolean(next.placement.crossSurface)&&['xMm','yMm','widthMm','heightMm','rotationDeg'].every(key=>previous.placement[key as keyof Layer['placement']]===next.placement[key as keyof Layer['placement']]))return next;
 const fits=(l:Layer)=>footprint(l).every(p=>contains(p.x,p.y));
 if(fits(next))return next;
 if(!fits(previous))return previous;
 let lo=0,hi=1;
 for(let i=0;i<12;i++){
  const t=(lo+hi)/2,p={...next.placement};
  for(const key of ['xMm','yMm','widthMm','heightMm','rotationDeg'] as const)p[key]=previous.placement[key]+(next.placement[key]-previous.placement[key])*t;
  if(fits({...next,placement:p}))lo=t;else hi=t;
 }
 const p={...next.placement};
 for(const key of ['xMm','yMm','widthMm','heightMm','rotationDeg'] as const)p[key]=previous.placement[key]+(next.placement[key]-previous.placement[key])*lo;
 return {...next,placement:p};
}
export function nextLayerName(layers:Layer[]):string {
 return '图层'+(layers.reduce((max,l)=>Math.max(max,Number(/^图层(\d+)$/.exec(l.name)?.[1]??0)),0)+1);
}
