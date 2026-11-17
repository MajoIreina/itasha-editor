import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Action, Settings, View, closeClean, desktop, execute, mutation, readView, optionGroups, optionLabels } from '../bridge/desktop';
import { Onboarding, Page } from '../features/onboarding/Onboarding';
import { SettingsPanel } from '../features/settings/SettingsPanel';
import { SplitWorkspace } from '../features/workspace/SplitWorkspace';
import { shortcutFromEvent, displayShortcut } from '../shared/input/shortcuts';
import { InspectorDock } from '../features/workspace/InspectorDock';
import { ProjectLibrary } from '../features/projects/ProjectLibrary';
import { NativeViewport } from '../features/viewport/NativeViewport';
import { DesignCanvas } from '../features/editor/DesignCanvas';
import { CanvasTools, LayerProperties } from '../features/editor/CanvasTools';
import { useCanvasEditor } from '../features/editor/useCanvasEditor';
import { invoke } from '@tauri-apps/api/core';
import { useTheme } from '../shared/theme/theme';
import '../shared/theme/theme.css';
import './styles.css';
import '../features/workspace/dock.css';
import '../features/onboarding/onboarding.css';

export function App() {
  const [view, setView] = useState<View | null>(null);
  useTheme(view?.settings.theme ?? 'dark');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState('未命名工程');
  const [closing, setClosing] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [page, setPage] = useState<Page>('welcome');
  const [splitDragging, setSplitDragging] = useState(false);
  const [pendingNavigation, setPendingNavigation] = useState<{ page: Page; id?: string } | null>(null);
  const active = useRef(false);
  const operationQueue = useRef(Promise.resolve());
  const project = view?.project;
  const refresh = async () => { const value = await readView(); setView(value); };
  const run = (operation: () => Promise<void>) => {
    const pending = operationQueue.current.then(async () => {
      active.current = true; setBusy(true); setError('');
      try { await operation(); await refresh(); return true; }
      catch (e) { setError(typeof e === 'object' && e && 'message' in e ? String(e.message) : String(e)); return false; }
      finally { active.current = false; setBusy(false); }
    });
    operationQueue.current = pending.then(() => undefined, () => undefined);
    return pending;
  };
  const act = (action: Action) => run(() => execute(action));
  const editor = useCanvasEditor(project ?? null, page === 'workspace', view?.settings.paths.models ?? '', view?.settings.performance.textureLimit ?? 4096, act, setError, refresh);
  const importAsset = async () => {
    if (!project || busy) return;
    await run(async () => { await invoke('import_image', { mutation: mutation(project) }); });
  };
  const performNavigation = async (target: Page, id?: string) => {
    if (id && !(await act({ type: 'open', id }))) return false;
    setPage(target); return true;
  };
  const navigate = async (target: Page, id?: string) => {
    if (busy || active.current) return false;
    if (project?.dirty && (target !== 'workspace' || id)) {
      setPendingNavigation({ page: target, id }); return false;
    }
    return performNavigation(target, id);
  };
  const resolveNavigation = async (mode: 'save' | 'discard') => {
    if (!project || !pendingNavigation) return;
    if (await act({ type: mode, mutation: mutation(project) })) {
      const target = pendingNavigation; setPendingNavigation(null);
      await performNavigation(target.page, target.id);
    }
  };
  useEffect(() => {
    if (!desktop) return;
    void run(refresh);
    const listening = listen('close-requested', () => { setSettingsOpen(false); setPendingNavigation(null); setClosing(true); });
    return () => { void listening.then(unlisten => unlisten()); };
  }, []);
  useEffect(() => { if (project) setName(project.name); }, [project?.name, project?.session.sessionEpoch]);
  useEffect(() => {
    const nativeListening = listen<string>('viewport-shortcut', event => {
      if (settingsOpen || closing || pendingNavigation || !view || busy) return;
      const action = (Object.keys(view.settings.shortcuts) as (keyof Settings['shortcuts'])[]).find(key => view.settings.shortcuts[key] === event.payload);
      if (action === 'settings') { setError(''); setSettingsOpen(true); }
      else if (action && project && page === 'workspace') void act({ type: action, mutation: mutation(project) });
    });
    const handler = (event: KeyboardEvent) => {
      if (event.defaultPrevented || settingsOpen || closing || pendingNavigation || !view) return;
      const shortcut = shortcutFromEvent(event);
      const action = (Object.keys(view.settings.shortcuts) as (keyof Settings['shortcuts'])[]).find(key => view.settings.shortcuts[key] === shortcut);
      if (!action) return;
      const target = event.target as HTMLElement;
      if ((target.closest('input,textarea,select') || target.isContentEditable) && (action === 'undo' || action === 'redo')) return;
      event.preventDefault();
      if (busy) return;
      if (action === 'settings') { setError(''); setSettingsOpen(true); }
      else if (project && page === 'workspace') void act({ type: action, mutation: mutation(project) });
    };
    window.addEventListener('keydown', handler);
    return () => { window.removeEventListener('keydown', handler); void nativeListening.then(unlisten => unlisten()); };
  }, [view, busy, settingsOpen, closing, page, pendingNavigation]);
  const close = (mode: 'save' | 'discard' | 'clean') => run(async () => {
    if (project && mode !== 'clean') await execute({ type: mode, mutation: mutation(project) });
    await closeClean();
  });
  const layout = view?.settings.layout ?? 'both';
  const direction = view?.settings.workspace.direction ?? 'vertical';
  const vehicle = view?.vehicles.find(v => v.id === project?.vehicleId);
  return <div className={'shell' + (view?.settings.performance.reducedMotion ? ' reduced-motion' : '')}>
    <header>
      <div className="brand"><b>痛</b><span>痛车编辑器 <small>NEXT / 0.3</small></span></div>
      <span className="project-title">{page === 'workspace' ? (project?.name ?? '工作台') + (project?.dirty ? ' · 未保存' : '') : page === 'welcome' ? '设计，从这里开始' : '设计向导'}</span>
      <button disabled={!view || busy} onClick={() => { setError(''); setSettingsOpen(true); }} title={view ? displayShortcut(view.settings.shortcuts.settings) : ''}>⚙ 设置</button>
      {page === 'workspace' && <button disabled={!project || busy} onClick={() => project && act({ type: 'save', mutation: mutation(project) })}>保存工程</button>}
    </header>
    {page === 'workspace' && <nav>
      <span className="stage">阶段 3 · 画布与贴花</span>
      <button disabled={busy} onClick={() => navigate('welcome')}>← 欢迎页</button>
      <button disabled={!desktop || busy} onClick={() => navigate('vehicles')}>＋ 新建设计</button>
      <button disabled={!project || busy} onClick={() => project && act({ type: 'undo', mutation: mutation(project) })}>撤销</button>
      <button disabled={!project || busy} onClick={() => project && act({ type: 'redo', mutation: mutation(project) })}>重做</button>
      <div className="spacer" />
      {(['both', '3d', 'edit'] as const).map((value, i) => <button key={value} className={layout === value ? 'selected' : ''} disabled={!view || busy} onClick={() => view && act({ type: 'settings', value: { ...view.settings, layout: value } })}>{['双视图', '3D 视图', '2D 画布'][i]}</button>)}
      <div className="direction-toggle" role="group" aria-label="双视图排列">{(['vertical', 'horizontal'] as const).map((value, i) => <button key={value} aria-pressed={direction === value} className={direction === value ? 'active-direction' : ''} disabled={!view || busy || layout !== 'both'} onClick={() => view && act({ type: 'settings', value: { ...view.settings, workspace: { ...view.settings.workspace, direction: value } } })}>{['上下', '左右'][i]}</button>)}</div>
    </nav>}
    {!desktop && <div className="notice">当前为浏览器预览。请运行 pnpm desktop:dev 使用 Rust 工程服务。</div>}
    {error && <div className="error" role="alert">{error}<button onClick={() => run(refresh)} disabled={busy}>重新读取</button></div>}
    {page === 'workspace' ? <main>
      <ProjectLibrary width={view?.settings.workspace.libraryWidth ?? 214} minimized={view?.settings.workspace.libraryMinimized ?? false} busy={busy}
        onChange={(libraryWidth, libraryMinimized) => view && void act({ type: 'settings', value: { ...view.settings, workspace: { ...view.settings.workspace, libraryWidth, libraryMinimized } } })}>
        <h2>工程库 <small>{view?.projects.length ?? 0}</small></h2>
        <p className="muted">本机草稿 · 独立于旧版数据</p>
        {view?.projects.map(item => <button className={'project-card ' + (project?.session.projectId === item.projectId ? 'current' : '')} key={item.projectId} disabled={busy} onClick={() => navigate('workspace', item.projectId)}><span>▧</span><strong>{item.name}</strong><small>修订 {item.revision}</small></button>)}
        {!view?.projects.length && <p className="empty-list">保存后的工程会显示在这里。</p>}
        {project?.dirty && <p className="muted">切换工程前请保存，或在右侧放弃修改。</p>}
        <div className="pipeline"><i /> Rust 工程服务<br /><small>React / TypeScript 界面</small></div>
      </ProjectLibrary>
      <SplitWorkspace layout={layout} direction={direction} percent={view?.settings.workspace.splitPercent ?? 65} busy={busy}
        onDragChange={setSplitDragging}
        onChange={splitPercent => view && void act({ type: 'settings', value: { ...view.settings, workspace: { ...view.settings.workspace, splitPercent } } })}>
        {layout !== 'edit' && <NativeViewport editor={editor} project={project ?? null} vehicleName={vehicle?.name ?? '通用车型'} suspended={settingsOpen || closing || Boolean(pendingNavigation)} modelDirectory={view?.settings.paths.models ?? ''} textureLimit={view?.settings.performance.textureLimit ?? 4096} passThrough={splitDragging} />}
        {layout !== '3d' && <DesignCanvas project={project ?? null} editor={editor} busy={busy} suspended={settingsOpen || closing || Boolean(pendingNavigation)} act={act} />}
      </SplitWorkspace>
      <InspectorDock panels={view?.settings.workspace.panels ?? {}} busy={busy} onChange={panels => view && void act({ type: 'settings', value: { ...view.settings, workspace: { ...view.settings.workspace, panels } } })}
        renderPanel={section => <CanvasTools section={section} project={project ?? null} editor={editor} busy={busy} onImport={() => { void importAsset(); }} />}><div className="dock-properties">
        {editor.selected && <LayerProperties key={editor.selected.id} layer={editor.selected} asset={project?.editor.assets.find(a => editor.selected?.source.kind === 'image' && a.id === editor.selected.source.assetId)} busy={busy || !editor.writable} onUpdate={layer => { void editor.edit({ type: 'update', layer }); }} />}
        <label>工程名称<input value={name} maxLength={100} disabled={!project || busy} onChange={e => setName(e.target.value)} /></label>
        <button className="wide" disabled={!project || busy || name === project.name} onClick={() => project && act({ type: 'rename', mutation: mutation(project), name })}>应用名称</button>
        <dl><dt>工程状态</dt><dd>{project ? project.dirty ? '未保存' : '已保存' : '未打开'}</dd><dt>当前修订</dt><dd>{project?.revision ?? '—'}</dd><dt>车型</dt><dd>{vehicle?.name ?? '未选择'}</dd></dl>
        {vehicle && project?.configuration && <div className="inspector-configuration"><h2>基础选配</h2>{optionGroups.map(key => <div key={key}><small>{optionLabels[key]}</small><span>{vehicle[key].find(o => o.id === project.configuration![key])?.name}</span></div>)}</div>}


        <button className="wide danger" disabled={!project?.dirty || busy} onClick={() => {
          if (project && window.confirm('放弃当前未保存的修改并关闭此工程？')) void act({ type: 'discard', mutation: mutation(project) }).then(ok => { if (ok) setPage('welcome'); });
        }}>放弃未保存修改</button>
      </div></InspectorDock>
    </main> : <Onboarding page={page} view={view} busy={busy} onPage={target => { void navigate(target); }} onOpen={id => navigate('workspace', id)} onCreate={async (name, vehicleId, configuration) => {
      const ok = await act({ type: 'create-configured', name, vehicleId, configuration });
      if (ok) setPage('workspace');
      return ok;
    }} onProjectAction={async (type, id, pinned) => {
      return act(type === 'pin-project' ? { type, id, pinned: Boolean(pinned) } : { type, id });
    }} />}
    <footer><span><i /> {busy ? '正在处理…' : desktop ? '桌面主框架就绪' : '界面预览'}</span><span>专用漆面展开 · 双端曲面编辑 · 施工导出待接入</span><span>本机工程 · Rust 管理</span></footer>
    {settingsOpen && view && <SettingsPanel settings={view.settings} dataDirectory={view.dataDirectory} busy={busy} saveError={error} onSave={value => act({ type: 'settings', value })} onClose={() => setSettingsOpen(false)} />}
    {pendingNavigation && !closing && <div className="modal-backdrop"><section className="modal" role="dialog" aria-modal="true" aria-labelledby="switch-title"><h2 id="switch-title">当前设计尚未保存</h2><p>保存或放弃当前修改后，继续切换页面。</p>{error && <p role="alert">{error}</p>}<div><button disabled={busy} onClick={() => setPendingNavigation(null)}>取消</button><button disabled={busy} onClick={() => resolveNavigation('discard')}>放弃并继续</button><button disabled={busy} className="selected" onClick={() => resolveNavigation('save')}>保存并继续</button></div></section></div>}
    {closing && <div className="modal-backdrop"><section role="dialog" aria-modal="true" aria-labelledby="close-title" className="modal">
      <h2 id="close-title">{project?.dirty ? '当前工程尚未保存' : '关闭编辑器？'}</h2>
      <p>{project?.dirty ? '保存后关闭，或明确放弃本次修改。' : '所有已保存的工程仍保留在本机。'}</p>
      {error && <p role="alert">{error}</p>}
      <div><button disabled={busy} onClick={() => setClosing(false)}>取消</button>
        {project?.dirty ? <><button disabled={busy} onClick={() => close('discard')}>放弃并关闭</button><button disabled={busy} className="selected" onClick={() => close('save')}>保存并关闭</button></> : <button disabled={busy} onClick={() => close('clean')}>关闭</button>}
      </div>
    </section></div>}
  </div>;
}
