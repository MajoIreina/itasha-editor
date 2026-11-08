const path = require('node:path');
const { BrowserWindow, session } = require('electron');
const { installSaveOnClose } = require('./lifecycle.cjs');

async function createEditorWindow(config, backend) {
  // This is an offline localhost application; bypass machine proxy settings.
  await session.defaultSession.setProxy({ mode: 'direct' });
  const window = new BrowserWindow({
    title: 'Itasha Editor', width: 1600, height: 1000, minWidth: 720, minHeight: 520,
    backgroundColor: '#171717', autoHideMenuBar: true, show: false,
    icon: path.join(config.root, 'app', 'icon.png'),
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'), contextIsolation: true,
      sandbox: true, nodeIntegration: false, webSecurity: true, devTools: Boolean(config.debugPort),
      // Hidden acceptance windows must use foreground timing for meaningful QA.
      backgroundThrottling: !config.testMode,
      additionalArguments: [`--itasha-host=${encodeURIComponent(JSON.stringify({ gpuAtStartup: config.gpuAtStartup, settingsError: config.settingsError, host: 'electron', version: '0.2.0' }))}`]
    }
  });
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.webContents.on('will-navigate', (event, url) => { if (!url.startsWith(backend.origin + '/')) event.preventDefault(); });
  window.webContents.on('will-attach-webview', event => event.preventDefault());
  window.on('page-title-updated', event => event.preventDefault());
  session.defaultSession.setPermissionRequestHandler((_, __, callback) => callback(false));
  session.defaultSession.webRequest.onBeforeRequest((details, callback) => {
    const allowed = details.url.startsWith(backend.origin + '/') || /^(data|blob|devtools):/.test(details.url);
    callback({ cancel: !allowed });
  });
  installSaveOnClose(window);
  window.once('ready-to-show', () => { if (!config.testMode) window.show(); });
  // Set the session before navigation, keeping credentials out of URLs and logs.
  await session.defaultSession.cookies.set({ url: backend.origin, name: 'itasha_session', value: backend.token, httpOnly: true, sameSite: 'strict', path: '/' });
  await window.loadURL(backend.origin + '/');
  window.setTitle('Itasha Editor');
  return window;
}
module.exports = { createEditorWindow };
