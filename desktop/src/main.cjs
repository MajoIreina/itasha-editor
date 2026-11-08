const { app, dialog } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { resolveConfig } = require('./config.cjs');
const { BackendProcess } = require('./backend.cjs');
const { createEditorWindow } = require('./window.cjs');

app.setName('痛车编辑器');
// HTTP header values must remain ASCII. Electron otherwise inserts the Chinese
// display name into User-Agent, which strict HTTP parsers reject before routing.
app.userAgentFallback = `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) ItashaEditor/0.2.0 Chrome/${process.versions.chrome} Electron/${process.versions.electron} Safari/537.36`;
const config = resolveConfig(app);
if (config.testMode) {
  // Windows otherwise treats the hidden QA window as fully occluded and
  // reduces compositor ticks to 1 Hz even with backgroundThrottling disabled.
  app.commandLine.appendSwitch('disable-features', 'CalculateNativeWinOcclusion');
  app.commandLine.appendSwitch('disable-renderer-backgrounding');
  app.commandLine.appendSwitch('disable-backgrounding-occluded-windows');
}
fs.mkdirSync(config.profile, { recursive: true });
app.setPath('userData', config.profile);
if (!config.gpuAtStartup) app.disableHardwareAcceleration();
if (config.debugPort) { app.commandLine.appendSwitch('remote-debugging-address', '127.0.0.1'); app.commandLine.appendSwitch('remote-debugging-port', String(config.debugPort)); }
const backend = new BackendProcess(config);
let window, quitting = false;

if (!app.requestSingleInstanceLock()) { app.quit(); }
else {
  app.on('second-instance', () => { if (window) { if (window.isMinimized()) window.restore(); window.show(); window.focus(); } });
  app.on('window-all-closed', () => app.quit());
  app.on('before-quit', event => {
    if (window && !window.isDestroyed()) { event.preventDefault(); window.close(); return; }
    if (!quitting) { event.preventDefault(); quitting = true; backend.stop().finally(() => app.quit()); }
  });
  app.on('child-process-gone', (_, details) => { if (details.type === 'GPU' && window && !window.isDestroyed()) window.webContents.executeJavaScript(`window.dispatchEvent(new CustomEvent('itasha-gpu-failed'))`).catch(() => {}); });
  app.whenReady().then(async () => {
    const ready = await backend.start((code) => {
      if (window && !window.isDestroyed() && !config.testMode) dialog.showErrorBox('后台服务已停止', `Rust 后端意外退出（${code}）。请重新启动软件；已保存工程可以继续打开。`);
    });
    window = await createEditorWindow(config, ready);
    fs.writeFileSync(path.join(config.data, '.itasha-session.json'), JSON.stringify({ host: 'electron', backend: 'rust', pid: process.pid, backendPid: ready.pid, port: ready.port, version: '0.2.0' }, null, 2));
    window.on('closed', () => { window = null; });
  }).catch(async error => {
    fs.mkdirSync(path.join(config.data, 'logs'), { recursive: true });
    fs.writeFileSync(path.join(config.data, 'logs', 'startup-error.log'), error.stack || error.message);
    await backend.stop();
    if (!config.testMode) dialog.showErrorBox('痛车编辑器启动失败', error.message);
    if (window && !window.isDestroyed()) window.destroy();
    quitting = true; app.quit();
  });
}
