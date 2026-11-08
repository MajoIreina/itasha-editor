// Assemble a portable application in this workspace. Never changes vehicle assets
// or overwrites projects/exports. Rust and Electron versions are lockfile-pinned.
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const crypto = require('node:crypto');
const root = path.resolve(__dirname, '..');
const desktop = path.join(root, 'desktop');
const native = path.join(desktop, 'native');
const electron = path.join(desktop, 'electron');

// Seed the bundled example only; never replace an existing user project.
const example = path.join(root, 'examples', '示例-蓝曜流线.su7design');
const exampleTarget = path.join(root, 'projects', '示例-蓝曜流线.su7design');
if (fs.existsSync(example) && !fs.existsSync(exampleTarget)) {
  fs.mkdirSync(path.dirname(exampleTarget), { recursive: true });
  fs.copyFileSync(example, exampleTarget, fs.constants.COPYFILE_EXCL);
}

if (!process.argv.includes('--skip-rust')) {
  execFileSync('cargo', ['build', '--locked', '--release', '--manifest-path', path.join(root, 'backend', 'Cargo.toml')], { cwd: root, stdio: 'inherit', windowsHide: true });
}
const distribution = path.join(desktop, 'node_modules', 'electron', 'dist');
if (!fs.existsSync(path.join(distribution, 'electron.exe'))) throw new Error('请先在 desktop 安装锁定的 Electron 依赖并运行 electron/install.js');
if (!fs.existsSync(path.join(native, 'imagemagick', 'magick.exe'))) throw new Error('缺少 desktop/native/imagemagick/magick.exe；参见 docs/开发与构建.md');
fs.mkdirSync(native, { recursive: true });
fs.copyFileSync(path.join(root, 'backend/target/release/itasha-backend.exe'), path.join(native, 'itasha-backend.exe'));
fs.copyFileSync(path.join(root, 'config/imagemagick-policy.xml'), path.join(native, 'imagemagick/policy.xml'));
fs.cpSync(distribution, electron, { recursive: true });
const app = path.join(electron, 'resources', 'app');
fs.mkdirSync(app, { recursive: true });
fs.copyFileSync(path.join(desktop, 'package.json'), path.join(app, 'package.json'));
fs.cpSync(path.join(desktop, 'src'), path.join(app, 'src'), { recursive: true });
fs.renameSync(path.join(electron, 'electron.exe'), path.join(electron, '痛车编辑器.exe'));
const legacy = path.join(root, 'development', 'legacy-webview2');
fs.mkdirSync(legacy, { recursive: true });
for (const name of ['痛车编辑器.exe', 'SU7涂装工作室.exe']) {
  const target = path.join(root, name);
  const backup = path.join(legacy, name);
  if (fs.existsSync(target) && !fs.existsSync(backup)) fs.copyFileSync(target, backup);
  fs.copyFileSync(path.join(root, 'backend/target/release/itasha-launcher.exe'), target);
}
const digest = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const manifest = {
  schemaVersion: 1, version: '0.2.0', builtAt: new Date().toISOString(),
  host: 'Electron', backend: 'Rust', runtimeDependencies: ['Electron', 'ImageMagick'],
  electron: JSON.parse(fs.readFileSync(path.join(desktop, 'node_modules/electron/package.json'), 'utf8')).version,
  imageMagick: execFileSync(path.join(native, 'imagemagick/magick.exe'), ['-version'], { encoding: 'utf8', windowsHide: true }).split(/\r?\n/)[0],
  backendSha256: digest(path.join(native, 'itasha-backend.exe')),
  launcherSha256: digest(path.join(root, '痛车编辑器.exe'))
};
fs.writeFileSync(path.join(desktop, 'build-manifest.json'), JSON.stringify(manifest, null, 2));
console.log(JSON.stringify(manifest, null, 2));
