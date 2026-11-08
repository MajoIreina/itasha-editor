const { test } = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');
const { resolveConfig } = require('../src/config.cjs');

test('packaged root resolves above desktop/electron/resources and validates ports', () => {
  const original = process.resourcesPath;
  process.resourcesPath = path.join('C:/portable/痛车编辑器', 'desktop/electron/resources');
  try {
    const config = resolveConfig({ isPackaged: true }, [], {});
    assert.equal(config.root, path.resolve('C:/portable/痛车编辑器'));
    assert.equal(config.port, 8878);
    assert.throws(() => resolveConfig({ isPackaged: true }, [], { ITASHA_PORT: '-1' }));
  } finally { process.resourcesPath = original; }
});

test('GPU settings and profile come from the chosen data directory', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'itasha-config-'));
  try {
    fs.writeFileSync(path.join(dir, 'performance-settings.json'), JSON.stringify({ gpuAcceleration: false }));
    const config = resolveConfig({ isPackaged: false }, ['--itasha-debug=9241'], { ITASHA_DATA_DIR: dir, ITASHA_PROFILE_DIR: path.join(dir, 'profile') });
    assert.equal(config.gpuAtStartup, false);
    assert.equal(config.debugPort, 9241);
    assert.equal(config.profile, path.join(dir, 'profile'));
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});
