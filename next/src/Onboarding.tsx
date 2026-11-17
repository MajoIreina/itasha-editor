import { useEffect, useRef, useState } from 'react';
import { OptionGroup, Vehicle, VehicleConfiguration, View, optionGroups, optionLabels } from './bridge';

export type Page = 'welcome' | 'load' | 'vehicles' | 'configure' | 'workspace';
type ProjectAction = 'delete-project' | 'duplicate-project' | 'pin-project';
function RecentProjectCard({ item, vehicleName, busy, onOpen, onAction }: {
  item: View['projects'][number]; vehicleName: string; busy: boolean;
  onOpen: (id: string) => Promise<boolean>;
  onAction: (action: ProjectAction, id: string, pinned?: boolean) => Promise<boolean>;
}) {
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const root = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return;
    const first = menuRef.current?.querySelector<HTMLButtonElement>('button');
    first?.focus();
    const dismiss = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenu(null);
    };
    const keyboard = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); setMenu(null); root.current?.focus(); }
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        const buttons = [...(menuRef.current?.querySelectorAll<HTMLButtonElement>('button') ?? [])];
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const step = event.key === 'ArrowDown' ? 1 : -1;
        buttons[(index + step + buttons.length) % buttons.length]?.focus(); event.preventDefault();
      }
    };
    document.addEventListener('pointerdown', dismiss);
    document.addEventListener('keydown', keyboard);
    return () => { document.removeEventListener('pointerdown', dismiss); document.removeEventListener('keydown', keyboard); };
  }, [menu]);
  useEffect(() => {
    if (!menu || !menuRef.current) return;
    const box = menuRef.current.getBoundingClientRect();
    const x = Math.max(8, Math.min(menu.x, window.innerWidth - box.width - 8));
    const y = Math.max(8, Math.min(menu.y, window.innerHeight - box.height - 8));
    if (x !== menu.x || y !== menu.y) setMenu({ x, y });
  }, [menu]);
  const showMenu = (x: number, y: number) => { setMenu({ x, y }); };
  const invoke = async (action: ProjectAction, pinned?: boolean) => {
    setMenu(null); await onAction(action, item.projectId, pinned);
  };
  return <>
    <button ref={root} className="recent-project-card" disabled={busy} onClick={() => onOpen(item.projectId)}
      onContextMenu={event => { event.preventDefault(); event.stopPropagation(); showMenu(event.clientX, event.clientY); }}
      onKeyDown={event => { if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) { event.preventDefault(); const box = event.currentTarget.getBoundingClientRect(); showMenu(box.left + 24, box.bottom); } }}
      aria-haspopup="menu" aria-expanded={Boolean(menu)}>
      <span className="file-symbol">▧</span><div><strong>{item.pinned && <span className="pin-indicator" aria-label="已置顶">◆ </span>}{item.name}</strong><small>{vehicleName} · 修订 {item.revision}</small></div><span>↗</span>
    </button>
    {menu && <div ref={menuRef} className="project-context-menu" role="menu" aria-label={`${item.name} 的操作`} style={{ left: menu.x, top: menu.y }}>
      <button role="menuitem" disabled={busy} onClick={() => void invoke('pin-project', !item.pinned)}>{item.pinned ? '取消置顶' : '置顶'}</button>
      <button role="menuitem" disabled={busy} onClick={() => void invoke('duplicate-project')}>复制</button>
      <div className="menu-separator" role="separator" />
      <button role="menuitem" className="menu-danger" disabled={busy} onClick={() => void invoke('delete-project')}>删除</button>
    </div>}
  </>;
}
export function VehicleMark({ style = 'sedan', color = 'var(--vehicle-line)' }: { style?: string; color?: string }) {
  const roof = style === 'mpv' ? 'M97 100 L117 43 Q124 30 147 30 L287 30 Q306 31 319 61 L337 103' : style === 'suv' ? 'M97 100 L145 47 Q154 39 174 39 L260 39 Q286 40 303 66 L330 103' : 'M95 100 L159 57 Q173 48 192 48 L247 48 Q268 50 291 72 L326 103';
  return <svg viewBox="0 0 440 180" className="vehicle-mark" aria-hidden="true">
    <ellipse cx="222" cy="153" rx="188" ry="10" fill="#000000" opacity=".15" />
    <path d={roof} fill={color} fillOpacity=".1" stroke={color} strokeWidth="2" />
    <path d="M32 107 Q58 93 100 95 L332 95 Q364 95 400 112 L405 135 L370 141 A31 31 0 0 0 309 141 L139 141 A31 31 0 0 0 78 141 L35 134 Z" fill={color} fillOpacity=".15" stroke={color} strokeWidth="2"/>
    <path d="M148 88 L175 57 L245 57 L280 88 Z" fill="var(--vehicle-glass)" stroke={color} strokeOpacity=".45"/>
    <path d="M218 57 L218 90 M165 103 L165 129 M297 105 L297 129 M42 112 L72 112 M376 114 L397 119" stroke={color} strokeOpacity=".55" strokeWidth="2"/>
    {[109,340].map(cx => <g key={cx}><circle cx={cx} cy="138" r="24" fill="var(--vehicle-wheel)" stroke={color} strokeWidth="2"/><circle cx={cx} cy="138" r="15" fill="none" stroke={color} strokeOpacity=".6"/><path d={'M' + cx + ' 123 v30 M' + (cx - 15) + ' 138 h30'} stroke={color} strokeOpacity=".6"/></g>)}
  </svg>;
}
function selectedOptions(vehicle: Vehicle, configuration: VehicleConfiguration) {
  return optionGroups.map(key => vehicle[key].find(option => option.id === configuration[key])!);
}
export function Onboarding({ page, view, busy, onPage, onOpen, onCreate, onProjectAction }: {
  page: Page; view: View | null; busy: boolean;
  onPage: (page: Page) => void; onOpen: (id: string) => Promise<boolean>;
  onCreate: (name: string, vehicleId: string, configuration: VehicleConfiguration) => Promise<boolean>;
  onProjectAction: (action: ProjectAction, id: string, pinned?: boolean) => Promise<boolean>;
}) {
  const [vehicleId, setVehicleId] = useState('');
  const [configuration, setConfiguration] = useState<VehicleConfiguration | null>(null);
  const [name, setName] = useState('');
  const [category, setCategory] = useState<OptionGroup>('paint');
  const [filter, setFilter] = useState('全部');
  const [search, setSearch] = useState('');
  const title = useRef<HTMLHeadingElement>(null);
  useEffect(() => { title.current?.focus(); }, [page]);
  const vehicle = view?.vehicles.find(v => v.id === vehicleId);
  const chooseVehicle = (value: Vehicle) => {
    if (value.id !== vehicleId) {
      setVehicleId(value.id); setConfiguration(structuredClone(value.defaults)); setName(value.name + ' 新设计'); setCategory('paint');
    }
  };
  const heading = page === 'welcome' ? '让灵感，成为车身的一部分。' : page === 'vehicles' ? '选择你的创作载体' : page === 'configure' ? '定义设计的起点' : '加载已有设计';
  const options = vehicle && configuration ? selectedOptions(vehicle, configuration) : [];
  const subtitle = page === 'welcome' ? '从一份新设计开始，或继续上次的创作。' : page === 'vehicles' ? '车型决定车身基础，下一步配置外观与座舱。' : page === 'configure' ? '选择车漆、轮毂、内饰与方向盘，再进入编辑工作区。' : '打开本机已保存的 Next 设计，继续编辑。';
  const recent = (view?.projects ?? []).filter(item => item.name.toLowerCase().includes(search.toLowerCase()));
  return <main className={'onboarding page-' + page}>
    <div className="onboarding-inner">
      <div className="flow-kicker">{page === 'welcome' || page === 'load' ? 'ITASHA / DESIGN STUDIO' : <><button disabled={busy} onClick={() => onPage('welcome')}>欢迎</button><span>/</span><span className={page === 'vehicles' ? 'current-step' : ''}>01 选择车型</span><span>/</span><span className={page === 'configure' ? 'current-step' : ''}>02 车型选配</span><span>/</span><span>03 工作区</span></>}</div>
      <h1 ref={title} tabIndex={-1}>{heading}</h1><p className="onboarding-subtitle">{subtitle}</p>
      {page === 'welcome' && <>
        <div className="welcome-actions">
          <button className="welcome-action primary-action" disabled={!view || busy} onClick={() => onPage('vehicles')}><span className="action-symbol">＋</span><strong>新建设计</strong><p>选择车型与基础配置，开启新创作</p><span className="action-link">开始设计 <b>↗</b></span></button>
          <button className="welcome-action" disabled={!view || busy} onClick={() => onPage('load')}><span className="action-symbol">▧</span><strong>加载设计</strong><p>从本机设计库，回到你的工作区</p><span className="action-link">浏览设计 <b>→</b></span></button>
          <div className="welcome-art"><VehicleMark /><span>YOUR VEHICLE. YOUR CANVAS.</span><small>形态示意 · 3D 引擎接入中</small></div>
        </div>
        {view?.project && <button className="continue-design" disabled={busy} onClick={() => onPage('workspace')}>继续当前设计：{view.project.name} {view.project.dirty ? '· 未保存' : ''}<span>返回工作区 →</span></button>}
        <div className="section-heading"><h2>最近设计</h2><span>{view?.projects.length ?? 0} 份本机设计</span></div>
        <div className="recent-designs">{view?.projects.slice(0,3).map(item => <RecentProjectCard key={item.projectId} item={item} busy={busy} onOpen={onOpen} onAction={onProjectAction} vehicleName={view.vehicles.find(v => v.id === item.vehicleId)?.name ?? '早期草稿'} />)}
          {!view?.projects.length && <div className="welcome-empty">还没有保存的设计。创建第一份设计后，它会出现在这里。</div>}</div>
      </>}
      {page === 'load' && <>
        <div className="library-tools"><button disabled={busy} onClick={() => onPage('welcome')}>← 返回欢迎页</button><input aria-label="搜索设计" placeholder="搜索设计名称…" value={search} onChange={e => setSearch(e.target.value)} /></div>
        {view?.projectWarnings.length ? <details className="library-warnings"><summary>未加载的工程（{view.projectWarnings.length}）</summary><ul>{view.projectWarnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul></details> : null}
        <div className="design-library">{recent.map(item => <button disabled={busy} key={item.projectId} onClick={() => onOpen(item.projectId)}><VehicleMark style={view?.vehicles.find(v => v.id === item.vehicleId)?.bodyStyle} />{item.pinned && <span className="library-pin">◆ 已置顶</span>}<strong>{item.name}</strong><small>{view?.vehicles.find(v => v.id === item.vehicleId)?.name ?? '早期无车型草稿'} · {new Date(Number(item.savedAtUnixMs)).toLocaleDateString('zh-CN')}</small><span>打开设计 →</span></button>)}</div>
        {!recent.length && <div className="library-empty"><span>▧</span><h2>{search ? '没有匹配的设计' : '设计库还是空的'}</h2><p>这里显示本机已保存的设计。无法读取的旧工程会列在此页上方。</p><button disabled={!view || busy} onClick={() => onPage('vehicles')}>新建设计</button></div>}
      </>}
      {page === 'vehicles' && <>
        <div className="vehicle-filters">{['全部', '轿车', 'SUV', '多功能车型'].map(value => <button disabled={busy} aria-pressed={filter === value} key={value} onClick={() => setFilter(value)}>{value}</button>)}<small>本机车型目录 · 配置数据已就绪</small></div>
        <div className="vehicle-grid">{view?.vehicles.filter(v => filter === '全部' || filter === v.family).map(v => <button disabled={busy} className={'vehicle-card' + (vehicleId === v.id ? ' is-selected' : '')} aria-pressed={vehicleId === v.id} key={v.id} onClick={() => chooseVehicle(v)}><div className="vehicle-card-head"><small>{v.family}</small><span className="selection-ring">{vehicleId === v.id ? '✓' : ''}</span></div><VehicleMark style={v.bodyStyle} color={vehicleId === v.id ? 'var(--accent)' : 'var(--vehicle-line)'} /><strong>{v.name}</strong><p>{v.paint.length} 种车漆 · {v.wheels.length} 种轮毂</p><small>车型形态示意 · 非实时模型</small></button>)}</div>
        <div className="flow-bottom"><span>{vehicle ? '已选择 ' + vehicle.name : '请选择一个车型继续'}</span><button disabled={busy} onClick={() => onPage('welcome')}>返回</button><button className="selected" disabled={!vehicle || busy} onClick={() => onPage('configure')}>下一步：车型选配 →</button></div>
      </>}
      {page === 'configure' && vehicle && configuration && <>
        <div className="configuration-layout"><section className="configuration-preview"><div className="preview-caption"><span>基础车型</span><small>形态示意 · 配置记录</small></div><VehicleMark style={vehicle.bodyStyle} /><h2>{vehicle.name}</h2><p>当前保存选配信息，实时模型与材质预览待引擎接入。</p><div className="configuration-summary">{optionGroups.map((key, i) => <div key={key}><small>{optionLabels[key]}</small><strong>{options[i]?.name}</strong></div>)}</div></section>
          <section className="configuration-controls"><label className="design-name">设计名称<input maxLength={100} disabled={busy} value={name} onChange={e => setName(e.target.value)} /></label><div className="configuration-tabs" role="tablist" aria-label="车型选配">{optionGroups.map(key => <button role="tab" aria-selected={category === key} aria-controls="vehicle-options" disabled={busy} key={key} onClick={() => setCategory(key)}>{optionLabels[key]}</button>)}</div><div id="vehicle-options" role="tabpanel" aria-label={optionLabels[category]} className="vehicle-options">
            <h3>选择{optionLabels[category]}</h3><p>{vehicle[category][0]?.id === 'factory' ? '此车型没有独立选装数据，保留原车配置。' : '配置来自本机车型目录，与设计一起保存。'}</p>
            {vehicle[category].map(option => <button key={option.id} disabled={busy} title={option.name} aria-pressed={configuration[category] === option.id} className={'vehicle-option' + (configuration[category] === option.id ? ' is-selected' : '')} onClick={() => setConfiguration(c => c && ({ ...c, [category]: option.id }))}><span className={'option-swatch ' + category} style={option.swatch ? { backgroundColor: option.swatch } : undefined}>{option.swatch ? '' : category === 'wheels' ? '◉' : category === 'steering' ? '⊕' : category === 'interior' ? '▦' : '◈'}</span><span><strong>{option.name}</strong></span><span className="selection-ring">{configuration[category] === option.id ? '✓' : ''}</span></button>)}
          </div></section></div>
        <div className="flow-bottom"><span>四项基础配置已选定，进入工作区后保存设计</span><button disabled={busy} onClick={() => onPage('vehicles')}>← 返回车型</button><button className="selected" disabled={busy || !name.trim()} onClick={() => onCreate(name, vehicle.id, configuration)}>{busy ? '正在创建…' : '完成配置，进入工作区 →'}</button></div>
      </>}
    </div>
  </main>;
}
