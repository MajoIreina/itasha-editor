(() => {
  const nativeGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function(type, options) {
    return nativeGetContext.call(this, type, /^webgl/.test(type) ? {...options, preserveDrawingBuffer:true} : options);
  };
  document.addEventListener('click', event => {
    if (location.pathname.endsWith('/select') && event.target.textContent.trim() === '总览') {
      const canvas = [...document.querySelectorAll('canvas')].sort((a,b)=>b.width*b.height-a.width*a.height)[0];
      try { if (canvas) sessionStorage.setItem('su7-preview-image', canvas.toDataURL('image/jpeg',0.88)); } catch {}
    }
  }, true);
  let scheduled = false;
  function cleanPrices() {
    scheduled = false;
    document.querySelectorAll('[class*="priceRowItem"],[class*="PriceRowItem"]').forEach(node => {
      if (node.style.display !== 'none') node.style.setProperty('display', 'none', 'important');
    });
    document.querySelectorAll('p,span,div,button').forEach(node => {
      if (node.classList.contains('local-hide-price') || node.children.length > 5) return;
      const direct = [...node.childNodes].filter(n => n.nodeType === Node.TEXT_NODE).map(n => n.textContent).join('').trim();
      if (/^[¥￥]$/.test(direct)) {
        const parent = node.closest('p') || node.parentElement;
        if (parent && parent.textContent.length < 100) parent.classList.add('local-hide-price');
      } else if (direct && /[¥￥]\s*[\d,]|选装券|套装优惠|权益优惠|权益赠送|价格|金融试算器|试算具体方案|车辆总价|保存心愿单/.test(direct) && node.textContent.length < 160) {
        node.classList.add('local-hide-price');
      }
    });
    document.querySelectorAll('iframe').forEach(node => node.classList.add('local-hide-price'));
    const preview = document.getElementById('local-vehicle-preview');
    if (location.pathname.endsWith('/preview') && !preview) {
      const data = sessionStorage.getItem('su7-preview-image');
      if (data) {
        const wrap=document.createElement('div');wrap.id='local-vehicle-preview';
        wrap.style.cssText='position:fixed;inset:0 auto 0 0;width:66.666667vw;height:100vh;overflow:hidden;z-index:2;pointer-events:none';
        const img=document.createElement('img');img.src=data;img.alt='当前车辆配置';
        img.style.cssText='width:100vw;height:100vh;max-width:none;object-fit:fill';
        wrap.append(img);document.body.append(wrap);
      }
    } else if (!location.pathname.endsWith('/preview') && preview) preview.remove();
    document.querySelectorAll('div.flex-col').forEach(node => {
      if (node.querySelector('p.local-hide-price') && node.textContent.length < 180) node.classList.add('local-hide-price');
    });
  }
  function schedule() {
    if (!scheduled) { scheduled = true; requestAnimationFrame(cleanPrices); }
  }
  new MutationObserver(schedule).observe(document.documentElement, {childList:true,subtree:true,characterData:true});
  schedule();
})();
