import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Project } from './bridge';
import type { CanvasEditor } from './useCanvasEditor';
import './viewport.css';

interface Status {
  state: string; message: string; sessionEpoch: string; adapter: string; backend: string;
  software: boolean; readyMs: number; frames: number;
  antialiasing: string; renderWidth: number; renderHeight: number; targetFps: number; sceneTimeMinutes: number;
  temporalSamples: number; temporalPending: boolean; renderScaleLimited: boolean;
  stats: { triangles: number; primitives: number; textures: number; parseMs: number } | null;
}
interface Bounds { x: number; y: number; width: number; height: number; visible: boolean; light: boolean; passThrough: boolean }
// Coalesce layout changes and keep the newest rectangle while one IPC call is in flight.
let latest: Bounds | null = null;
let sending = false;
async function sendBounds(bounds: Bounds) {
  latest = bounds;
  if (sending) return;
  sending = true;
  try {
    while (latest) {
      const next = latest; latest = null;
      await invoke('viewport_bounds', { bounds: next });
    }
  } catch (error) { console.error('Native viewport bounds:', error); }
  finally { sending = false; }
}
const message = (error: unknown) => typeof error === 'object' && error && 'message' in error ? String(error.message) : String(error);
const antialiasingLabels: Record<string, string> = { off: '无抗锯齿', fxaa: 'FXAA', taa: 'TAA', taa2x: 'TAA 2x', ssaa: 'SSAA 2×' };

export function NativeViewport({ project, vehicleName, suspended, modelDirectory, textureLimit, editor, passThrough = false }: {
  project: Project | null; vehicleName: string; suspended: boolean; modelDirectory: string; textureLimit: number; editor:CanvasEditor; passThrough?: boolean;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);
  const epoch = project?.session.sessionEpoch;
  useEffect(() => {
    let disposed = false;
    const receive = (value: Status) => { if (!disposed && value.sessionEpoch === epoch) setStatus(value); };
    const listening = listen<Status>('viewport-status', event => receive(event.payload));
    setStatus(null); setError('');
    if (project) {
      void invoke('viewport_load', { session: project.session, force: attempt > 0 })
        .then(() => invoke<Status>('viewport_status')).then(receive)
        .catch(e => { if (!disposed) setError(message(e)); });
    }
    return () => { disposed = true; void listening.then(unlisten => unlisten()); };
  }, [epoch, attempt, modelDirectory, textureLimit]);

  const ready = !error && status?.state === 'ready';
  useEffect(()=>{
    const element=host.current;if(!element||!ready)return;
    return editor.registerDropTarget('native',{element,locate:async(x,y)=>{
      if(suspended||!editor.writable)return null;
      const r=element.getBoundingClientRect(),scale=window.devicePixelRatio;
      const hit=await invoke<{surfaceId:string;localPoint:[number,number]}|null>('viewport_probe',{x:(x-r.x)*scale,y:(y-r.y)*scale});
      if(!hit)return null;
      const p=hit.localPoint;
      return {surfaceId:hit.surfaceId,xMm:p[0],yMm:p[1],cross:editor.crossMode};
    }});
  },[ready,suspended,editor.writable,editor.crossMode,project?.session.sessionEpoch]);
  // Pass-through toggles per drag step; it must update the native window without
  // tearing down the bounds effect (whose cleanup hides the viewport).
  const passThroughRef = useRef(passThrough); passThroughRef.current = passThrough;
  const resendBounds = useRef<() => void>(() => {});
  useLayoutEffect(() => {
    let frame = 0;
    const update = () => {
      frame = 0;
      const rect = host.current?.getBoundingClientRect();
      const scale = window.devicePixelRatio;
      void sendBounds({ x: Math.round((rect?.x ?? 0) * scale), y: Math.round((rect?.y ?? 0) * scale),
        width: Math.round((rect?.width ?? 0) * scale), height: Math.round((rect?.height ?? 0) * scale),
        visible: Boolean(ready && !suspended && !document.hidden && rect && rect.width > 1 && rect.height > 1),
        light: document.documentElement.dataset.theme === 'light', passThrough: passThroughRef.current });
    };
    resendBounds.current = update;
    const schedule = () => { if (!frame) frame = requestAnimationFrame(update); };
    const resize = new ResizeObserver(schedule);
    if (host.current) resize.observe(host.current);
    const theme = new MutationObserver(schedule);
    theme.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    window.addEventListener('resize', schedule);
    window.addEventListener('scroll', schedule, true);
    document.addEventListener('visibilitychange', schedule);
    update();
    return () => {
      cancelAnimationFrame(frame); resize.disconnect(); theme.disconnect();
      window.removeEventListener('resize', schedule); window.removeEventListener('scroll', schedule, true);
      document.removeEventListener('visibilitychange', schedule);
      void sendBounds({ x: 0, y: 0, width: 1, height: 1, visible: false, light: false, passThrough: false });
    };
  }, [ready, suspended]);
  useEffect(() => { resendBounds.current(); }, [passThrough]);
  const failure = error || (status?.state === 'error' ? status.message : '');
  return <section className="viewport native-preview" aria-label="原生 3D 预览">
    <div className="panel-title"><span>3D 预览 · {vehicleName}</span><div className="viewport-actions"><small>{ready ? `${status.backend} · ${status.software ? '软件适配器' : '原生 GPU'} · ${antialiasingLabels[status.antialiasing] ?? status.antialiasing}` : 'Rust / wgpu'}</small><label className="viewport-cross-surface" title="素材沿已连接的相邻漆面连续显示；关闭后仅显示在当前部件"><input type="checkbox" checked={editor.crossMode} disabled={!editor.writable||suspended||Boolean(editor.selected?.locked)} onChange={e=>void editor.toggleCross(e.target.checked)}/>跨相邻漆面</label><button disabled={!ready} onClick={()=>{void invoke('viewport_focus').catch(e=>setError(message(e)));}}>定位选中素材</button><button disabled={!ready} onClick={() => { void invoke('viewport_reset').catch(e => setError(message(e))); }}>复位视角</button></div></div>
    <div className="native-viewport-host" ref={host}>
      {!ready && <div className="native-placeholder" role={failure ? 'alert' : 'status'}><div className="wire-cube">◇</div><h1>{failure ? '3D 预览暂不可用' : project ? '正在载入车型' : '请先打开设计'}</h1><p>{failure || '读取模块化 GLB、材质与贴图…'}</p>{failure && <button onClick={() => setAttempt(value => value + 1)}>重新载入模型</button>}</div>}
    </div>
    <div className="viewport-status" title={status?.adapter}><span>{ready && status.stats ? `${status.stats.triangles.toLocaleString()} 三角面 · ${status.stats.primitives} 图元 · ${Math.round(status.readyMs)} ms 载入${status.renderScaleLimited ? ' · 分辨率受显存预算限制' : ''}` : '标准 GLB · 独立资产管线'}</span><span>点选/拖动素材 · 空白处旋转 · Alt+左键旋转 · 右键平移 · 滚轮缩放</span></div>
  </section>;
}
