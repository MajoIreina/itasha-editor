import type { EditorDocument, Layer } from './bridge';
export interface SurfaceInfo { id: string; name: string; profileVersion: number; widthMm: number; heightMm: number; triangles: number; projection: number[][]; depth: number }
export interface Surface extends SurfaceInfo { mask: Path2D }
export interface CoverageFragment { partId:string; paintUv:[number,number][]; imageUv:[number,number][] }
export function drawCoverage(ctx:CanvasRenderingContext2D,layer:Layer,bitmap:HTMLCanvasElement,fragments:CoverageFragment[],part:string) {
  if(!layer.visible)return;
  for(const f of fragments) {
    if(f.partId!==part)continue;
    const p=f.paintUv, uv=f.imageUv.map(([u,v])=>[(layer.placement.flipX?1-u:u)*bitmap.width,v*bitmap.height]);
    const ax=uv[1][0]-uv[0][0],ay=uv[1][1]-uv[0][1],bx=uv[2][0]-uv[0][0],by=uv[2][1]-uv[0][1];
    const det=ax*by-ay*bx;if(Math.abs(det)<1e-9)continue;
    const dx=p[1][0]-p[0][0],dy=p[1][1]-p[0][1],ex=p[2][0]-p[0][0],ey=p[2][1]-p[0][1];
    const a=(dx*by-ex*ay)/det,b=(dy*by-ey*ay)/det,c=(ex*ax-dx*bx)/det,d=(ey*ax-dy*bx)/det;
    ctx.save();ctx.beginPath();ctx.moveTo(...p[0]);ctx.lineTo(...p[1]);ctx.lineTo(...p[2]);ctx.closePath();ctx.clip();
    ctx.transform(a,b,c,d,p[0][0]-a*uv[0][0]-c*uv[0][1],p[0][1]-b*uv[0][0]-d*uv[0][1]);
    ctx.globalAlpha=layer.opacity;ctx.drawImage(bitmap,0,0);ctx.restore();
  }
}
/** Source-image coordinates before the optional visual flip, shared by picking and drag grabs. */
export function coveragePoint(layer:Layer,bitmap:HTMLCanvasElement,fragments:CoverageFragment[],part:string,x:number,y:number):{x:number;y:number}|null {
  if(!layer.visible||layer.opacity<=0)return null;
  for(const f of fragments) {
    if(f.partId!==part)continue;
    const p=f.paintUv,a=[p[1][0]-p[0][0],p[1][1]-p[0][1]],b=[p[2][0]-p[0][0],p[2][1]-p[0][1]],q=[x-p[0][0],y-p[0][1]];
    const det=a[0]*b[1]-a[1]*b[0];if(Math.abs(det)<1e-9)continue;
    const v=(q[0]*b[1]-q[1]*b[0])/det,w=(a[0]*q[1]-a[1]*q[0])/det,u=1-v-w;
    if(Math.min(u,v,w)<-1e-5)continue;
    const imageX=f.imageUv[0][0]*u+f.imageUv[1][0]*v+f.imageUv[2][0]*w;
    let ix=imageX;
    const iy=f.imageUv[0][1]*u+f.imageUv[1][1]*v+f.imageUv[2][1]*w;
    if(layer.placement.flipX)ix=1-ix;
    if(bitmap.getContext('2d')!.getImageData(Math.max(0,Math.min(bitmap.width-1,Math.floor(ix*bitmap.width))),Math.max(0,Math.min(bitmap.height-1,Math.floor(iy*bitmap.height))),1,1).data[3]>12)return {x:imageX,y:iy};
  }
  return null;
}
export function coverageHit(layer:Layer,bitmap:HTMLCanvasElement,fragments:CoverageFragment[],part:string,x:number,y:number) {
  return coveragePoint(layer,bitmap,fragments,part,x,y)!==null;
}
export function imageGrabOffset(layer:Layer,point:{x:number;y:number}) {
  return {x:(point.x-.5)*layer.placement.widthMm,y:(point.y-.5)*layer.placement.heightMm};
}
export function maskPath(triangles: Float32Array, info: SurfaceInfo) {
  const path = new Path2D();
  for (let i = 0; i < triangles.length; i += 6) {
    path.moveTo(triangles[i] * info.widthMm, triangles[i + 1] * info.heightMm);
    path.lineTo(triangles[i + 2] * info.widthMm, triangles[i + 3] * info.heightMm);
    path.lineTo(triangles[i + 4] * info.widthMm, triangles[i + 5] * info.heightMm); path.closePath();
  }
  return path;
}
export function drawLayer(ctx: CanvasRenderingContext2D, layer: Layer, images: Map<string, HTMLImageElement>) {
  if (!layer.visible) return;
  const p = layer.placement;
  ctx.save(); ctx.translate(p.xMm, p.yMm); ctx.rotate(p.rotationDeg * Math.PI / 180);if(p.flipX)ctx.scale(-1,1);
  ctx.globalAlpha = layer.opacity;
  const w = p.widthMm, h = p.heightMm, source = layer.source;
  if (source.kind === 'image') {
    const image = images.get(source.assetId); if (image) ctx.drawImage(image, -w / 2, -h / 2, w, h);
  } else if (source.kind === 'text') {
    ctx.fillStyle = source.fill; ctx.font = '100px "' + source.fontFamily + '"';
    ctx.textBaseline = 'top'; ctx.textAlign = 'center';
    const lines = source.text.split('\n');
    const maxWidth = Math.max(1, ...lines.map(line => ctx.measureText(line).width));
    ctx.scale(w / maxWidth, h / (lines.length * 120)); ctx.translate(0, -lines.length * 60);
    lines.forEach((line, i) => ctx.fillText(line, 0, i * 120));
  } else {
    ctx.fillStyle = source.fill; ctx.strokeStyle = source.fill;
    ctx.beginPath();
    if (source.shape === 'rectangle') ctx.rect(-w / 2, -h / 2, w, h);
    else if (source.shape === 'ellipse') ctx.ellipse(0, 0, w / 2, h / 2, 0, 0, Math.PI * 2);
    else if (source.shape === 'line') { ctx.rect(-w / 2, -h / 2, w, h); }
    else {
      const count = source.shape === 'triangle' ? 3 : source.shape === 'hexagon' ? 6 : 10;
      for (let i = 0; i < count; i++) {
        const angle = -Math.PI / 2 + i * Math.PI * 2 / count;
        const radius = source.shape === 'star' && i % 2 ? 0.42 : 1;
        const x = Math.cos(angle) * w / 2 * radius, y = Math.sin(angle) * h / 2 * radius;
        if (!i) ctx.moveTo(x, y); else ctx.lineTo(x, y);
      }
      ctx.closePath();
    }
    ctx.fill();
  }
  ctx.restore();
}
export function composite(editor: EditorDocument, surface: Surface, images: Map<string, HTMLImageElement>, preview?: Layer) {
  const canvas = document.createElement('canvas');
  const scale = 1024 / Math.max(surface.widthMm, surface.heightMm);
  canvas.width = Math.max(1, Math.round(surface.widthMm * scale));
  canvas.height = Math.max(1, Math.round(surface.heightMm * scale));
  const ctx = canvas.getContext('2d')!;
  ctx.scale(canvas.width / surface.widthMm, canvas.height / surface.heightMm);
  ctx.clip(surface.mask);
  editor.layers.filter(layer => layer.placement.surfaceId === surface.id).forEach(layer => drawLayer(ctx, preview?.id === layer.id ? preview : layer, images));
  return canvas;
}
export function localPoint(layer: Layer, x: number, y: number) {
  const p = layer.placement, a = -p.rotationDeg * Math.PI / 180;
  const dx = x - p.xMm, dy = y - p.yMm;
  return { x: dx * Math.cos(a) - dy * Math.sin(a), y: dx * Math.sin(a) + dy * Math.cos(a) };
}

export function artworkBitmap(layer: Layer, images: Map<string, HTMLImageElement>) {
  const canvas = document.createElement('canvas');
  const image = layer.source.kind === 'image' ? images.get(layer.source.assetId) : null;
  const aspect = image ? image.naturalWidth / image.naturalHeight : layer.source.kind === 'text' ? 3 : 1;
  const solid=layer.source.kind==='shape'&&['rectangle','line'].includes(layer.source.shape);
  const limit=solid?1:image?1024:512;
  canvas.width = aspect >= 1 ? limit : Math.max(1, Math.round(limit * aspect));
  canvas.height = aspect >= 1 ? Math.max(1, Math.round(limit / aspect)) : limit;
  drawLayer(canvas.getContext('2d')!, {...layer, visible: true, opacity: 1,
    placement: {...layer.placement, xMm: canvas.width/2, yMm: canvas.height/2, widthMm: canvas.width, heightMm: canvas.height, rotationDeg: 0,flipX:false}}, images);
  return canvas;
}
export function alphaHit(layer: Layer, x: number, y: number, images: Map<string, HTMLImageElement>) {
  const p=localPoint(layer,x,y), w=layer.placement.widthMm,h=layer.placement.heightMm;
  if (Math.abs(p.x)>w/2 || Math.abs(p.y)>h/2 || layer.opacity<=0) return false;
  const bitmap=artworkBitmap(layer,images);const px=Math.min(bitmap.width-1,Math.floor(((layer.placement.flipX?-p.x:p.x)/w+.5)*bitmap.width)),py=Math.min(bitmap.height-1,Math.floor((p.y/h+.5)*bitmap.height));
  return bitmap.getContext('2d')!.getImageData(px,py,1,1).data[3]>12;
}
export function drawSelectionEdge(ctx: CanvasRenderingContext2D, layer: Layer, images: Map<string,HTMLImageElement>, scale: number) {
  const bitmap=artworkBitmap(layer,images), outline=document.createElement('canvas');outline.width=bitmap.width;outline.height=bitmap.height;
  const brush=outline.getContext('2d')!;brush.drawImage(bitmap,0,0);brush.globalCompositeOperation='source-in';brush.fillStyle='#1686ff';brush.fillRect(0,0,outline.width,outline.height);
  const p=layer.placement;ctx.save();ctx.translate(p.xMm,p.yMm);ctx.rotate(p.rotationDeg*Math.PI/180);if(p.flipX)ctx.scale(-1,1);
  for (const [x,y] of [[-2,0],[2,0],[0,-2],[0,2],[-1,-1],[1,1]]) ctx.drawImage(outline,-p.widthMm/2+x/scale,-p.heightMm/2+y/scale,p.widthMm,p.heightMm);
  ctx.restore();drawLayer(ctx,layer,images);
}
