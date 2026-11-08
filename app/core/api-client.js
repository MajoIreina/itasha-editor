// The only editor module that knows the HTTP transport and request headers.
(() => {
  function prepare(route, args = {}) {
    if (!/^[a-z][a-z-]*$/.test(route)) throw new Error('接口名称无效');
    // Snapshot now, before a queued save waits for another disk write. Encoding
    // once avoids a stringify/parse clone followed by another stringify.
    const body = JSON.stringify(args);
    return () => send(route, body);
  }
  async function send(route, body) {
    const response = await fetch('/__api/' + route, {
      method: 'POST', credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json', 'X-Studio-Request': '1' },
      body
    });
    const result = await response.json();
    if (!response.ok) throw new Error(result.error || '操作失败，请重试');
    return result;
  }
  const request = (route, args = {}) => prepare(route, args)();
  window.ItashaAPI = Object.freeze({ version: 1, request, prepare });
})();
