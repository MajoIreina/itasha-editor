const fs = require('node:fs');
const path = require('node:path');

function resolveConfig(app, argv = process.argv, env = process.env) {
  const root = path.resolve(env.ITASHA_ROOT || (app.isPackaged ? path.join(process.resourcesPath, '..', '..', '..') : path.join(__dirname, '..', '..')));
  const data = path.resolve(env.ITASHA_DATA_DIR || root);
  const profile = path.resolve(env.ITASHA_PROFILE_DIR || path.join(data, 'app-data-electron'));
  let gpuAtStartup = true, settingsError = '';
  try {
    const file = path.join(data, 'performance-settings.json');
    if (fs.existsSync(file)) {
      const settings = JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, ''));
      if (typeof settings.gpuAcceleration !== 'boolean') throw new Error('GPU 设置格式无效');
      gpuAtStartup = settings.gpuAcceleration;
    }
  } catch (error) { settingsError = `无法读取 GPU 设置，使用默认设置：${error.message}`; }
  const debugArg = argv.find(v => /^--itasha-debug(?:=\d+)?$/.test(v));
  const debugPort = debugArg ? Number(debugArg.split('=')[1] || 9233) : 0;
  const port = Number(env.ITASHA_PORT ?? 8878);
  if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error('后端端口无效');
  if (!Number.isInteger(debugPort) || debugPort < 0 || debugPort > 65535) throw new Error('调试端口无效');
  return { root, data, profile, gpuAtStartup, settingsError, debugPort, port, testMode: env.ITASHA_TEST_MODE === '1' };
}
module.exports = { resolveConfig };
