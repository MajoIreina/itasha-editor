import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Layer, LayerSource, Project, EditOperation, Action } from './bridge';
import { mutation } from './bridge';
import {nextLayerName} from './transform';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { readView } from './bridge';
import type { ImageAsset } from './bridge';
export interface DropPoint {surfaceId:string;xMm:number;yMm:number;cross?:boolean}
type DropTarget={element:HTMLElement;locate:(x:number,y:number)=>Promise<DropPoint|null>|DropPoint|null};
import { artworkBitmap, maskPath, Surface, SurfaceInfo } from './canvasEngine';
interface RendererState { state: string; message: string; generation: number; sessionEpoch: string; stats: { surfaceFingerprint: string;  } | null; surfaces: SurfaceInfo[] }
const errorText = (e: unknown) => typeof e === 'object' && e && 'message' in e ? String(e.message) : String(e);
export function useCanvasEditor(project: Project | null, active: boolean, modelDirectory: string, textureLimit: number,
  act: (action: Action) => Promise<boolean>, onError: (message: string) => void, refresh:()=>Promise<void>) {
  const [renderer, setRenderer] = useState<RendererState | null>(null);
  const [surfaces, setSurfaces] = useState<Surface[]>([]);
  const [surfaceId, setSurfaceId] = useState('su7.front-left-door');
  const [crossMode,setCrossMode]=useState(true);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [images, setImages] = useState(new Map<string, HTMLImageElement>());
  const [sync, setSync] = useState('等待外板映射');
  const epoch = project?.session.sessionEpoch;
  const selectionEpoch = useRef(epoch);
  const live = useRef(project); live.current = project;
  const urls = useRef(new Map<string,string>());
  useEffect(() => {
    selectionEpoch.current = epoch;
    setSelectedId(null); setCrossMode(true); setImages(new Map()); setSurfaces([]); setRenderer(null); setSync('等待外板映射');
    return () => { urls.current.forEach(URL.revokeObjectURL); urls.current.clear(); };
  }, [epoch]);
  useEffect(() => {
    if (!active || !project) return;
    let disposed = false;
    const receive = (s: RendererState) => {
      if (!disposed && s.sessionEpoch === epoch) setRenderer(old => old?.state === s.state && old.generation === s.generation ? old : s);
    };
    const pending = listen<RendererState>('viewport-status', event => receive(event.payload));
    const mapping = listen<string>('paint-mapping-updated', event => {
      if (!disposed && event.payload === epoch) void refresh();
    });
    void invoke<boolean>('viewport_load', { session: project.session, force: false })
      .then(async upgraded => { if (upgraded && !disposed) await refresh(); return invoke<RendererState>('viewport_status'); }).then(receive).catch(e => { if (!disposed) setSync(errorText(e)); });
    return () => { disposed = true; void pending.then(fn => fn()); void mapping.then(fn => fn()); };
  }, [epoch, active, modelDirectory, textureLimit]);
  useEffect(() => {
    if (renderer?.state !== 'ready' || !project) { setSurfaces([]); return; }
    let disposed = false;
    void Promise.all(renderer.surfaces.map(async info => {
      const bytes = await invoke<ArrayBuffer>('viewport_surface', { session: project.session, id: info.id });
      return { ...info, mask: maskPath(new Float32Array(bytes), info) };
    })).then(values => { if (!disposed) { setSurfaces(values); setSurfaceId(old => values.some(s => s.id === old) ? old : values[0]?.id ?? ''); } })
      .catch(e => { if (!disposed) onError(errorText(e)); });
    return () => { disposed = true; };
  }, [epoch, renderer?.state, renderer?.generation]);
  const assetKey = project?.editor.assets.map(a => a.id).join(',');
  useEffect(() => {
    if (!project) return;
    let disposed = false;
    for(const [id,url]of urls.current)if(!project.editor.assets.some(a=>a.id===id)){URL.revokeObjectURL(url);urls.current.delete(id);}
    void Promise.all(project.editor.assets.map(async asset => {
      if (images.has(asset.id)) return [asset.id, images.get(asset.id)!] as const;
      const data = await invoke<ArrayBuffer>('asset_bytes', { session: project.session, id: asset.id });
      if (disposed) throw Error('cancelled');
      const url = URL.createObjectURL(new Blob([data], { type: 'image/png' })); urls.current.set(asset.id,url);
      const image = new Image(); image.src = url; await image.decode(); if(disposed){URL.revokeObjectURL(url);if(urls.current.get(asset.id)===url)urls.current.delete(asset.id);} return [asset.id, image] as const;
    })).then(pairs => { if (!disposed) setImages(new Map(pairs)); }).catch(e => { if (!disposed) onError(errorText(e)); });
    return () => { disposed = true; };
  }, [epoch, assetKey]);
  const dropTargets=useRef(new Map<string,DropTarget>());
  const registerDropTarget=(id:string,target:DropTarget)=>{dropTargets.current.set(id,target);return()=>{if(dropTargets.current.get(id)===target)dropTargets.current.delete(id);};};
  const dropBusy=useRef(false);
  const importQueue=useRef(Promise.resolve());
  const [importing,setImporting]=useState(false);
  const fingerprint = renderer?.stats?.surfaceFingerprint;
  const mismatch = Boolean(project?.editor.modelFingerprint && fingerprint && project.editor.modelFingerprint !== fingerprint);
  const writable = Boolean(project && renderer?.state === 'ready' && surfaces.length && !mismatch);
  // One binary transfer in flight, one newest pending snapshot; never stream pointer moves.
  const pending = useRef<(() => Promise<void>) | null>(null);
  const running = useRef(false);
  const sent = useRef(new Map<string, string>());
  useEffect(() => { sent.current.clear(); }, [epoch, renderer?.generation]);
  useEffect(()=>{for(const id of sent.current.keys())if(!project?.editor.layers.some(l=>l.id===id))sent.current.delete(id);},[project?.revision]);
  useEffect(() => {
    if (!active || !project || !writable || project.editor.assets.some(a => !images.has(a.id))) return;
    let disposed = false;
    const timer = setTimeout(() => {
      pending.current = async () => {
        const snapshot = project;
        setSync('正在同步曲面贴花…');
        for (const layer of snapshot.editor.layers) {
          if (disposed || live.current?.session.sessionEpoch !== epoch || live.current?.revision !== snapshot.revision) return;
          const key=JSON.stringify(layer.source);
          if(sent.current.get(layer.id)===key)continue;
          const canvas=artworkBitmap(layer,images),data=canvas.getContext('2d')!.getImageData(0,0,canvas.width,canvas.height).data;
          await invoke('viewport_decal',data.buffer,{headers:{
            'x-project':snapshot.session.projectId,'x-session':snapshot.session.sessionEpoch,'x-revision':snapshot.revision,
            'x-layer':layer.id,'x-width':String(canvas.width),'x-height':String(canvas.height),
          }});
          if(!disposed)sent.current.set(layer.id,key);
        }
        for(const id of sent.current.keys())if(!snapshot.editor.layers.some(l=>l.id===id))sent.current.delete(id);
        if (!disposed) setSync('3D 曲面贴花已同步');
      };
      if (!running.current) {
        running.current = true;
        void (async () => {
          try { while (pending.current) {
            const task = pending.current; pending.current = null;
            try { await task(); } catch (e) { if (live.current?.session.sessionEpoch === epoch) setSync('同步失败：' + errorText(e)); }
          } }
          finally { running.current = false; }
        })();
      }
    }, 80);
    return () => { disposed = true; clearTimeout(timer); };
  }, [project?.revision, epoch, writable, images, surfaces, active, renderer?.generation]);
  useEffect(()=>{
    if(!project || !active)return;
    void invoke('viewport_select',{session:project.session,id:project.editor.layers.some(l=>l.id===selectedId)?selectedId:null}).catch(()=>{});
  },[epoch,selectedId,active,renderer?.generation,project?.revision]);
  useEffect(()=>{
    const selection=listen<{sessionEpoch:string;id:string|null}>('viewport-selection',e=>{if(e.payload.sessionEpoch===epoch)setSelectedId(e.payload.id);});
    const edit=listen<{sessionEpoch:string;revision:string;layer:Layer}>('viewport-edit',e=>{
      const current=live.current;if(!current || e.payload.sessionEpoch!==current.session.sessionEpoch || e.payload.revision!==current.revision)return;
      void act({type:'edit',mutation:mutation(current),operation:{type:'update',layer:e.payload.layer}}).then(ok=>{if(!ok)void invoke('viewport_pointer',{input:{kind:'cancel',x:0,y:0}});});
    });
    return()=>{void selection.then(f=>f());void edit.then(f=>f());};
  },[epoch]);
  const [preview,setPreview]=useState<Layer|null>(null);
  const gestureRef=useRef<{stamp:{sessionEpoch:string;baseRevision:string;gestureId:string;mappingVersion:number};sequence:number}|null>(null);
  const gestureQueue=useRef(Promise.resolve());
  const sendGesture=(kind:string,layer?:Layer)=>{
    const g=gestureRef.current;if(!g)return Promise.resolve();
    if(kind==='preview')g.sequence++;
    const input={kind,stamp:g.stamp,sequence:g.sequence,layer:layer??null};
    const pending=gestureQueue.current.then(()=>invoke<void>('viewport_gesture',{input}));
    gestureQueue.current=pending.catch(e=>{onError(errorText(e));setPreview(null);});
    return pending;
  };
  const beginGesture=(layer:Layer)=>{
    if(!project)return Promise.resolve();
    gestureRef.current={stamp:{sessionEpoch:project.session.sessionEpoch,baseRevision:project.revision,gestureId:crypto.randomUUID(),mappingVersion:2},sequence:0};
    return sendGesture('begin',layer).catch(()=>{});
  };
  const latestPreview=useRef<Layer|null>(null);
  const previewPump=useRef<Promise<void>|null>(null);
  const previewGesture=(layer:Layer)=>{
    latestPreview.current=layer;
    if(!previewPump.current)previewPump.current=(async()=>{
      await Promise.resolve();
      while(latestPreview.current&&gestureRef.current){const value=latestPreview.current;latestPreview.current=null;await sendGesture('preview',value);}
    })().catch(()=>{}).finally(()=>{previewPump.current=null;});
    return previewPump.current;
  };
  const commitGesture=async()=>{await previewPump.current;try{await sendGesture('commit');}catch{}finally{gestureRef.current=null;}};
  const cancelGesture=async()=>{latestPreview.current=null;await previewPump.current;try{await sendGesture('cancel');}catch{}finally{gestureRef.current=null;setPreview(null);}};
  useEffect(()=>{setPreview(null);gestureRef.current=null;},[epoch,project?.revision]);
  useEffect(()=>{
    const pending=listen<{stamp:{sessionEpoch:string;baseRevision:string};layer:Layer|null}>('viewport-preview',e=>{
      const current=live.current;if(current&&e.payload.stamp.sessionEpoch===current.session.sessionEpoch&&(!e.payload.layer||e.payload.stamp.baseRevision===current.revision))setPreview(e.payload.layer);
    });return()=>{void pending.then(f=>f());};
  },[epoch]);
  const surface = surfaces.find(s => s.id === surfaceId) ?? null;
  const selected = selectionEpoch.current === epoch ? project?.editor.layers.find(l => l.id === selectedId) ?? null : null;
  useEffect(() => { if (selected) {setSurfaceId(selected.placement.surfaceId);setCrossMode(Boolean(selected.placement.crossSurface));} }, [selected?.id,selected?.placement.crossSurface,selected?.placement.surfaceId]);
  const edit = (operation: EditOperation) => {
    if(!project)return Promise.resolve(false);
    return act({type:'edit',mutation:mutation(project),operation});
  };
  const insert = async (source: LayerSource, _name?: string, drop?:DropPoint, snapshot=live.current, imported?:ImageAsset) => {
    const target=drop?surfaces.find(s=>s.id===drop.surfaceId):surface;
    if (!snapshot || !target || !writable || !fingerprint || snapshot.session.sessionEpoch!==epoch) return;
    const asset=imported??(source.kind==='image'?snapshot.editor.assets.find(a=>a.id===source.assetId):null);
    const ratio=asset?asset.height/asset.width:source.kind==='text'?.3:source.kind==='shape'&&source.shape==='line'?.04:2/3;
    const width=Math.min(300,target.widthMm*.6,target.heightMm*.6/ratio);
    let layer:Layer={id:crypto.randomUUID(),name:nextLayerName(snapshot.editor.layers),visible:true,locked:false,opacity:1,source,
      placement:{surfaceId:target.id,crossSurface:false,aspectRatioLocked:source.kind==='image',profileVersion:2,xMm:drop?.xMm??target.widthMm/2,yMm:drop?.yMm??target.heightMm/2,widthMm:width,heightMm:Math.max(1,width*ratio),rotationDeg:0}};
    layer.placement.crossSurface=drop?.cross??crossMode;
    // Find an editable anchor, without shrinking artwork to fit the panel outline.
    const ctx=document.createElement('canvas').getContext('2d')!;
    if(!ctx.isPointInPath(target.mask,layer.placement.xMm,layer.placement.yMm)) {
      const points=[];
      for(let y=1;y<40;y++)for(let x=1;x<64;x++) {
        const px=target.widthMm*x/64,py=target.heightMm*y/40;
        if(ctx.isPointInPath(target.mask,px,py))points.push({x:px,y:py,d:Math.hypot(px-layer.placement.xMm,py-layer.placement.yMm)});
      }
      points.sort((a,b)=>a.d-b.d);
      if(!points.length){onError('部件没有有效漆面锚点。');return;}
      layer.placement.xMm=points[0].x;layer.placement.yMm=points[0].y;
    }
    if(await act({type:'edit',mutation:mutation(snapshot),operation:{type:'insert',layer,fingerprint:snapshot.editor.modelFingerprint??fingerprint}})){
      setSurfaceId(target.id);setSelectedId(layer.id);
    }
  };
  const locateDrop=async(x:number,y:number)=>{
    for(const target of dropTargets.current.values()){
      const r=target.element.getBoundingClientRect();if(x>=r.left&&x<=r.right&&y>=r.top&&y<=r.bottom)return await target.locate(x,y);
    }
    return null;
  };
  const dropAsset=async(assetId:string,x:number,y:number)=>{
    if(dropBusy.current||!writable)return;dropBusy.current=true;
    try{const target=await locateDrop(x,y);if(target)await insert({kind:'image',assetId},undefined,target);}
    catch(e){onError(errorText(e));}finally{dropBusy.current=false;}
  };
  const importDropped=(paths:string[],point:DropPoint|null)=>{
    if(!active||!writable||!live.current)return Promise.resolve();
    const sessionEpoch=live.current.session.sessionEpoch;
    const task=async()=>{
    if(live.current?.session.sessionEpoch!==sessionEpoch)return;
    dropBusy.current=true;setImporting(true);
    try{
      for(const path of paths){
        let snapshot=(await readView()).project;
        if(!snapshot||snapshot.session.sessionEpoch!==sessionEpoch)break;
        const asset=await invoke<ImageAsset>('import_image_path',{mutation:mutation(snapshot),path});
        await refresh();snapshot=(await readView()).project;
        if(point&&snapshot?.session.sessionEpoch===sessionEpoch)await insert({kind:'image',assetId:asset.id},undefined,point,snapshot,asset);
      }
    }catch(e){onError(errorText(e));}finally{try{await refresh();}catch(e){onError(errorText(e));}dropBusy.current=false;setImporting(false);}
    };
    importQueue.current=importQueue.current.then(task,task);
    return importQueue.current;
  };
  const dropHandlers=useRef({locateDrop,importDropped});dropHandlers.current={locateDrop,importDropped};
  useEffect(()=>{
    if(!active||!project)return;
    const files=getCurrentWebview().onDragDropEvent(e=>{
      if(e.payload.type!=='drop')return;
      const {paths,position}=e.payload;
      void (async()=>{try{const p=await dropHandlers.current.locateDrop(position.x/window.devicePixelRatio,position.y/window.devicePixelRatio);await dropHandlers.current.importDropped(paths,p);}catch(e){onError(errorText(e));}})();
    });
    const native=listen<{sessionEpoch:string;paths:string[];point:DropPoint|null}>('viewport-file-drop',e=>{
      if(e.payload.sessionEpoch===epoch)void dropHandlers.current.importDropped(e.payload.paths,e.payload.point);
    });
    return()=>{void files.then(f=>f());void native.then(f=>f());};
  },[active,epoch]);
  const toggleCross=async(value:boolean)=>{setCrossMode(value);if(selected&&surface&&!selected.locked)await edit({type:'update',layer:toCross(selected,value,surface)});};
  return { preview,beginGesture,previewGesture,commitGesture,cancelGesture,crossMode,toggleCross,renderer, surfaces, surface, surfaceId, setSurfaceId, selected, selectedId, setSelectedId, images, writable: writable&&!importing, importing, registerDropTarget, dropAsset, insert, edit, nextName:()=>nextLayerName(project?.editor.layers??[]),
    message: mismatch ? '模型几何指纹与工程不一致，已停止映射。' : renderer?.state === 'error' ? renderer.message : sync };
}
export type CanvasEditor = ReturnType<typeof useCanvasEditor>;

function toCross(layer:Layer,cross:boolean,_surface:Surface):Layer {
 return {...layer,placement:{...layer.placement,crossSurface:cross}};
}
