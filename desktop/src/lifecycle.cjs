const { dialog } = require('electron');

function installSaveOnClose(window) {
  let approved = false, saving = false;
  window.webContents.on('will-prevent-unload', event => { if (approved) event.preventDefault(); });
  window.on('close', event => {
    if (approved || window.webContents.isDestroyed()) return;
    event.preventDefault();
    if (saving) return;
    saving = true;
    (async () => {
      try {
        let timeout;
        try {
          const result = await Promise.race([
            window.webContents.executeJavaScript(`window.itashaLifecycle ? window.itashaLifecycle.prepareClose() : Promise.resolve({ok:true})`),
            new Promise((_, reject) => { timeout = setTimeout(() => reject(new Error('保存超过 60 秒，请检查磁盘空间后重试')), 60000); })
          ]);
          if (!result?.ok) throw new Error(result?.error || '工程保存失败');
        } finally { clearTimeout(timeout); }
        approved = true; window.close();
      } catch (error) {
        const { response } = await dialog.showMessageBox(window, { type: 'error', title: '工程尚未保存', message: error.message, buttons: ['继续编辑', '重试保存并退出', '放弃本次保存并退出'], defaultId: 0, cancelId: 0 });
        if (response === 2) { approved = true; window.destroy(); }
        else if (response === 1) { saving = false; window.close(); }
      } finally { saving = false; }
    })();
  });
}
module.exports = { installSaveOnClose };
