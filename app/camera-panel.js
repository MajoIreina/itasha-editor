/* Camera controls in the editor shell. The iframe owns all camera state. */
(() => {
  'use strict';
  const $=id=>document.getElementById(id),viewer=$('viewer'),area=document.querySelector('.viewer-area');
  const bridge=()=>window.studio?.bridge;
  const open=document.createElement('button');open.id='camera-open';open.textContent='◎';open.title='视角与镜头';open.setAttribute('aria-label','视角与镜头');open.setAttribute('aria-expanded','false');document.querySelector('.viewport-tools').append(open);
  const readout=document.createElement('button');readout.id='camera-readout';readout.title='打开镜头设置 · Alt + 滚轮调整 FOV';readout.textContent='FOV 30°';area.append(readout);
  const panel=document.createElement('section');panel.id='camera-panel';panel.hidden=true;panel.setAttribute('aria-label','视角与镜头设置');
  panel.innerHTML=`<div class="detail-heading"><strong>视角与镜头</strong><button id="camera-close" type="button" aria-label="关闭镜头设置">×</button></div>
    <div class="camera-content"><label class="camera-fov-label" for="camera-fov">视场角 FOV <span><input id="camera-fov-value" type="number" min="15" max="90" step="1" value="30" aria-label="FOV 角度"> °</span></label>
    <input id="camera-fov" type="range" min="15" max="90" step="0.1" value="30" aria-label="FOV 视场角">
    <div class="camera-lenses"><button data-camera-fov="20">20°</button><button data-camera-fov="35">35°</button><button data-camera-fov="50">50°</button><button data-camera-fov="70">70°</button></div>
    <label class="camera-framing"><input id="camera-preserve" type="checkbox" checked> 调整 FOV 时保持主体大小</label>
    <p class="camera-lens-note">小角度压缩透视，大角度增强近大远小。保持大小时会补偿相机距离。</p>
    <div class="camera-actions"><button id="camera-fit">适应整车</button><button id="camera-focus">聚焦素材</button><button id="camera-reset">重置镜头</button></div>
    <dl class="camera-help"><dt>旋转</dt><dd>左键拖动</dd><dt>平移</dt><dd>右键 / 中键 / Shift + 左键</dd><dt>靠近 / 远离</dt><dd>滚轮 · 朝向鼠标位置</dd><dt>调整 FOV</dt><dd>Alt + 滚轮</dd><dt>聚焦</dt><dd>双击车身 · F 聚焦素材</dd><dt>整车归位</dt><dd>Home · 保留 FOV 与方向</dd></dl>
    <p id="camera-status" role="status"></p></div>`;
  area.append(panel);
  const toggle=()=>{if(!bridge()?.cameraState)return;panel.hidden=!panel.hidden;open.setAttribute('aria-expanded',String(!panel.hidden));if(!panel.hidden){$('lighting-panel').hidden=true;refresh();}};
  open.onclick=readout.onclick=toggle;$('camera-close').onclick=()=>{panel.hidden=true;open.setAttribute('aria-expanded','false');};
  $('lighting-open')?.addEventListener('click',()=>{panel.hidden=true;open.setAttribute('aria-expanded','false');});
  function refresh(state=bridge()?.cameraState){
    const enabled=!!state;for(const button of [open,readout,...panel.querySelectorAll('input,button')])button.disabled=!enabled;if(!state)return;
    $('camera-fov').value=state.fov;$('camera-fov-value').value=+(state.fov.toFixed(1));$('camera-preserve').checked=state.preserveFraming;
    readout.textContent=`FOV ${+state.fov.toFixed(1)}°`;
    $('camera-status').textContent=`距离 ${state.distance.toFixed(2)} m · 水平 ${Math.round(state.yaw)}° / 俯仰 ${Math.round(state.pitch)}°`;
    document.querySelectorAll('[data-view]').forEach(b=>b.classList.toggle('active',b.dataset.view===state.view));
  }
  const setFov=value=>{if(Number.isFinite(value))refresh(bridge()?.setCamera({fov:value}));else refresh();};
  $('camera-fov').oninput=e=>setFov(Number(e.target.value));$('camera-fov-value').onchange=e=>setFov(e.target.value.trim()?Number(e.target.value):NaN);
  $('camera-preserve').onchange=e=>refresh(bridge()?.setCamera({preserveFraming:e.target.checked}));
  panel.querySelectorAll('[data-camera-fov]').forEach(b=>b.onclick=()=>setFov(Number(b.dataset.cameraFov)));
  $('camera-fit').onclick=()=>bridge()?.fitView();$('camera-reset').onclick=()=>bridge()?.resetView();
  $('camera-focus').onclick=()=>{if(!bridge()?.focusArtwork())$('camera-status').textContent='先选择一张位于车身上的素材，再聚焦。';};
  $('tool-select').title='适应整车 · Home';$('tool-select').onclick=()=>bridge()?.fitView();
  const renderMode=()=>{const current=$('render-current').checked;for(const id of ['render-yaw','render-pitch','render-fov'])$(id).disabled=current;};
  $('render-current').onchange=renderMode;
  $('high-res-render').addEventListener('click',()=>{const state=bridge()?.cameraState;if(!state)return;for(const key of ['yaw','pitch','fov'])$('render-'+key).value=+state[key].toFixed(2);renderMode();});
  window.addEventListener('message',e=>{if(e.source!==viewer.contentWindow||e.origin!==location.origin)return;if(e.data?.type==='su7-camera-change')refresh(e.data.state);else if(e.data?.type==='su7-ready')refresh();});
  // Navigation commands work while the viewport or its controls have focus.
  window.addEventListener('keydown',e=>{if(document.querySelector('dialog[open]')||e.ctrlKey||e.metaKey||e.altKey||/INPUT|TEXTAREA|SELECT/.test(e.target.tagName)||!area.contains(document.activeElement))return;if(e.key==='Home'){e.preventDefault();bridge()?.fitView();}else if(e.key.toLowerCase()==='f'){e.preventDefault();$('camera-focus').click();}});
  refresh();
})();
