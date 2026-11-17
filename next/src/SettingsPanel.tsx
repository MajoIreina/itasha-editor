import { useEffect, useRef, useState } from 'react';
import { Settings, chooseDirectory } from './bridge';
import { displayShortcut, shortcutFromEvent } from './shortcuts';

const tabs = ['工作区', '性能', '场景', '快捷键', '路径'] as const;
type Tab = typeof tabs[number];
const antialiasingDescriptions: Record<Settings['performance']['antialiasing'], string> = {
  off: '关闭边缘平滑，适合优先降低 GPU 开销。',
  fxaa: '轻量边缘平滑，细节可能略微柔化。',
  taa: '通过抖动采样与历史帧融合稳定边缘；停止操作后短暂累积收敛。',
  taa2x: '在所选渲染分辨率基础上，宽、高各 ×2（像素量 ×4），再进行时域采样；GPU 与显存开销较高。',
  ssaa: '在所选渲染分辨率基础上，宽、高各 ×2（像素量 ×4），再缩小输出；GPU 与显存开销较高。',
};
export function SettingsPanel({ settings, dataDirectory, busy, saveError, onSave, onClose }: {
  settings: Settings; dataDirectory: string; busy: boolean; saveError: string;
  onSave: (value: Settings) => Promise<boolean>; onClose: () => void;
}) {
  const [draft, setDraft] = useState<Settings>(() => structuredClone(settings));
  const [tab, setTab] = useState<Tab>('工作区');
  const [error, setError] = useState('');
  const [picking, setPicking] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.showModal();
    closeButton.current?.focus();
    return () => { previous?.focus(); };
  }, []);
  const locked = busy || picking;
  const changed = JSON.stringify(draft) !== JSON.stringify(settings);
  const close = () => {
    if (!locked && (!changed || window.confirm('设置尚未应用，放弃本次设置修改？'))) onClose();
  };
  const path = async (key: keyof Settings['paths']) => {
    setPicking(true); setError('');
    try {
      const directory = await chooseDirectory();
      if (directory !== null) setDraft(value => ({ ...value, paths: { ...value.paths, [key]: directory } }));
    } catch (e) { setError(String(e)); }
    finally { setPicking(false); }
  };
  const save = async () => {
    if (new Set(Object.values(draft.shortcuts)).size !== 4) { setError('快捷键存在冲突，请为每项设置不同的组合。'); return; }
    if (await onSave(draft)) onClose();
    else setError('设置未保存。请检查参数、快捷键和路径，修改后重试。');
  };
  const performance = (key: keyof Settings['performance'], value: number | boolean | string) => setDraft(s => ({ ...s, performance: { ...s.performance, [key]: value } }));
  return <dialog ref={dialog} className="settings-dialog" aria-labelledby="settings-title" onCancel={e => { e.preventDefault(); close(); }}>
    <div className="settings-top"><div><h2 id="settings-title">设置</h2><p>配置你的工作区与本机偏好</p></div><button ref={closeButton} disabled={locked} aria-label="关闭设置" onClick={close}>✕</button></div>
    <div className="settings-body">
      <div role="tablist" aria-label="设置分类" className="settings-tabs">{tabs.map(t => <button role="tab" key={t} aria-selected={t === tab} aria-controls="settings-content" disabled={locked} onClick={() => setTab(t)}>{t}</button>)}</div>
      <section className="settings-content" id="settings-content" role="tabpanel" aria-label={tab}>
        {tab === '工作区' && <>
          <h3>外观主题</h3>
          <label className="setting-row"><span>主题 <small>默认深色；跟随系统会实时切换</small></span><select aria-label="主题" disabled={locked} value={draft.theme} onChange={e => setDraft(s => ({ ...s, theme: e.target.value as Settings['theme'] }))}><option value="dark">深色（白字）</option><option value="light">浅色（黑字）</option><option value="system">跟随系统</option></select></label>
          <h3>视图排列</h3><p className="settings-hint">双视图支持上下和左右排列。也可以拖动视图之间的分隔条。</p>
          <div className="layout-options">{(['vertical', 'horizontal'] as const).map((direction, i) => <button disabled={locked} className={draft.workspace.direction === direction ? 'chosen' : ''} key={direction} onClick={() => setDraft(s => ({ ...s, workspace: { ...s.workspace, direction } }))}><div className={'layout-icon ' + direction}><span>3D</span><span>2D</span></div>{['上下排列', '左右排列'][i]}</button>)}</div>
          <label className="setting-row"><span>启动视图</span><select aria-label="启动视图" disabled={locked} value={draft.layout} onChange={e => setDraft(s => ({ ...s, layout: e.target.value as Settings['layout'] }))}><option value="both">双视图</option><option value="3d">3D 视图</option><option value="edit">2D 画布</option></select></label>
          <label className="setting-row"><span>3D 视图占比 <small>{draft.workspace.splitPercent}%</small></span><input disabled={locked} aria-label="3D 视图占比" type="range" min="25" max="75" value={draft.workspace.splitPercent} onChange={e => setDraft(s => ({ ...s, workspace: { ...s.workspace, splitPercent: Number(e.target.value) } }))} /></label>
        </>}
        {tab === '性能' && <>
          <h3>渲染与资源预算</h3><p className="settings-hint">抗锯齿、目标帧率和渲染分辨率应用后生效；静止并完成时域采样收敛后停止绘制。降低分辨率可减少像素着色负载。纹理上限重新载入模型时生效，最多 4096 px；缓存预算仍为预留偏好。</p>
          <label className="setting-row"><span>抗锯齿 <small id="antialiasing-description">{antialiasingDescriptions[draft.performance.antialiasing]}</small></span><select aria-label="抗锯齿" aria-describedby="antialiasing-description" disabled={locked} value={draft.performance.antialiasing} onChange={e => performance('antialiasing', e.target.value)}><option value="off">关闭</option><option value="fxaa">FXAA（默认）</option><option value="taa">TAA</option><option value="taa2x">TAA 2x</option><option value="ssaa">SSAA（2×）</option></select></label>
          <label className="setting-row"><span>目标帧率</span><select aria-label="目标帧率" disabled={locked} value={draft.performance.targetFps} onChange={e => performance('targetFps', Number(e.target.value))}>{[30,60,120].map(v => <option key={v} value={v}>{v} FPS</option>)}</select></label>
          <label className="setting-row"><span>渲染分辨率</span><select aria-label="渲染分辨率" disabled={locked} value={draft.performance.renderScale} onChange={e => performance('renderScale', Number(e.target.value))}>{[50,75,100].map(v => <option key={v} value={v}>{v}%</option>)}</select></label>
          <label className="setting-row"><span>纹理边长上限</span><select aria-label="纹理边长上限" disabled={locked} value={draft.performance.textureLimit} onChange={e => performance('textureLimit', Number(e.target.value))}>{[1024,2048,4096,8192].map(v => <option key={v} value={v}>{v} px</option>)}</select></label>
          <label className="setting-row"><span>资源缓存预算 <small>256–16,384 MB</small></span><input disabled={locked} type="number" aria-label="资源缓存预算" min="256" max="16384" step="256" value={draft.performance.cacheBudgetMb} onChange={e => performance('cacheBudgetMb', Number(e.target.value))} /></label>
          <label className="setting-row"><span>减少界面动态效果 <small>立即生效；也遵循系统减少动态效果偏好</small></span><input disabled={locked} type="checkbox" checked={draft.performance.reducedMotion} onChange={e => performance('reducedMotion', e.target.checked)} /></label>
        </>}
        {tab === '场景' && <>
          <h3>天空与光照</h3><p className="settings-hint">程序化天空盒随相机方向显示；时间控制天空、太阳方向及车身光照。场景偏好保存在本机，不修改设计图层。关闭天空盒仍保留时间光照。</p>
          <label className="setting-row"><span>天空盒</span><input aria-label="天空盒" type="checkbox" disabled={locked} checked={draft.scene.skybox} onChange={e => setDraft(s => ({...s, scene: {...s.scene, skybox:e.target.checked}}))}/></label>
          <label className="setting-row"><span>场景时间 <small>{String(Math.floor(draft.scene.timeMinutes/60)).padStart(2,'0')}:{String(draft.scene.timeMinutes%60).padStart(2,'0')}</small></span><input aria-label="场景时间" type="range" min="0" max="1439" step="1" disabled={locked} value={draft.scene.timeMinutes} onChange={e => setDraft(s => ({...s, scene: {...s.scene, timeMinutes:Number(e.target.value)}}))}/></label>
          <label className="setting-row"><span>具体时间</span><input aria-label="具体时间" type="time" disabled={locked} value={String(Math.floor(draft.scene.timeMinutes/60)).padStart(2,'0')+':'+String(draft.scene.timeMinutes%60).padStart(2,'0')} onChange={e=>{if(!e.target.value)return;const [h,m]=e.target.value.split(':').map(Number);setDraft(s=>({...s,scene:{...s.scene,timeMinutes:h*60+m}}));}}/></label>
          <div className="layout-options">{[[360,'日出'],[720,'正午'],[1080,'日落'],[0,'午夜']].map(([time,label])=><button disabled={locked} key={time} onClick={()=>setDraft(s=>({...s,scene:{...s.scene,timeMinutes:Number(time)}}))}>{label}</button>)}</div>
          <p className="settings-hint">相机限制在地平线上方，并限制最远距离与平移范围。太阳轨迹为展示用途，不是按地理位置计算的天文时间。</p>
        </>}
        {tab === '快捷键'  && <>
          <h3>按键绑定</h3><p className="settings-hint">点选输入框后按下 Ctrl（可加 Shift）与字母或逗号。文本输入期间保留原生编辑快捷键。</p>
          {(['save', 'undo', 'redo', 'settings'] as const).map((key, i) => <label className="setting-row" key={key}><span>{['保存工程', '撤销', '重做', '打开设置'][i]}</span><input disabled={locked} aria-label={['保存工程快捷键', '撤销快捷键', '重做快捷键', '设置快捷键'][i]} className="shortcut-input" readOnly value={displayShortcut(draft.shortcuts[key])} onKeyDown={e => {
            if (e.key === 'Tab' || e.key === 'Escape') return;
            e.preventDefault();
            const value = shortcutFromEvent(e);
            if (value) { setError(''); setDraft(s => ({ ...s, shortcuts: { ...s.shortcuts, [key]: value } })); }
          }} /></label>)}
          <button disabled={locked} onClick={() => setDraft(s => ({ ...s, shortcuts: { save: 'Ctrl+S', undo: 'Ctrl+Z', redo: 'Ctrl+Shift+Z', settings: 'Ctrl+Comma' } }))}>恢复默认按键</button>
        </>}
        {tab === '路径' && <>
          <h3>本机目录</h3><label className="path-setting">工程与设置目录<input readOnly value={dataDirectory} /></label><p className="settings-hint">模型目录可选择 3D 或 Blender/software；留空会在程序所在工作区查找。工程目录当前固定，导出与缓存目录仍为后续模块预留。</p>
          {(['models', 'exports', 'cache'] as const).map((key, i) => <div className="path-setting" key={key}><label htmlFor={'path-' + key}>{['模型资源目录', '默认导出目录', '资源缓存目录'][i]}</label><div><input id={'path-' + key} disabled={locked} value={draft.paths[key]} placeholder={dataDirectory + '\\' + key} onChange={e => setDraft(s => ({ ...s, paths: { ...s.paths, [key]: e.target.value } }))} /><button disabled={locked} type="button" aria-label={'浏览' + ['模型资源目录', '默认导出目录', '资源缓存目录'][i]} onClick={() => path(key)}>浏览…</button><button disabled={locked || !draft.paths[key]} aria-label={'重置' + ['模型资源目录', '默认导出目录', '资源缓存目录'][i]} onClick={() => setDraft(s => ({ ...s, paths: { ...s.paths, [key]: '' } }))}>重置</button></div></div>)}
          <p className="settings-hint">保存时检查目录存在且可读取，导出与缓存目录还会检查写入权限；不会移动或清空文件。</p>
        </>}
      </section>
    </div>
    <div className="settings-bottom"><span role={error || saveError ? 'alert' : undefined}>{saveError || error || (changed ? '有尚未应用的修改' : '设置已与本机同步')}</span><button disabled={locked} onClick={close}>取消</button><button className="selected" disabled={locked || !changed} onClick={save}>{busy ? '正在保存…' : '应用设置'}</button></div>
  </dialog>;
}
