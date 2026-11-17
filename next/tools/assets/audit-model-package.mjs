import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const root = fs.realpathSync(process.argv[2] ?? path.resolve(workspace, '../../3D/Blender/software'));
const manifest = JSON.parse(fs.readFileSync(path.join(root, 'config.json')));
const files = ['config.json', manifest.body_asset, ...manifest.wheel.map(v => v.asset), ...manifest.steering.map(v => v.asset), ...fs.readdirSync(path.join(root, 'textures')).filter(v => v.endsWith('.png')).map(v => 'textures/' + v)];
const records = [];
for (const relative of [...new Set(files)]) {
  if (path.isAbsolute(relative) || relative.split(/[\\/]/).some(v => v === '..')) throw Error('Invalid package path: ' + relative);
  const file = fs.realpathSync(path.join(root, relative));
  if (!file.startsWith(root + path.sep)) throw Error('Asset escapes package: ' + relative);
  const sha = createHash('sha256');
  for await (const chunk of fs.createReadStream(file)) sha.update(chunk);
  const record = { path: relative, bytes: fs.statSync(file).size, sha256: sha.digest('hex') };
  if (relative.endsWith('.glb')) {
    const bytes = fs.readFileSync(file);
    if (bytes.readUInt32LE(0) !== 0x46546c67 || bytes.readUInt32LE(4) !== 2 || bytes.readUInt32LE(8) !== bytes.length) throw Error('Invalid GLB: ' + relative);
    const doc = JSON.parse(bytes.subarray(20, 20 + bytes.readUInt32LE(12)).toString());
    record.meshes = doc.meshes.length; record.materials = doc.materials.length;
    record.primitives = doc.meshes.reduce((sum, m) => sum + m.primitives.length, 0);
    record.embeddedImages = doc.images?.filter(v => v.bufferView !== undefined).length ?? 0;
    record.extensions = doc.extensionsUsed ?? [];
  }
  records.push(record);
}
const report = { generatedAt: new Date().toISOString(), sourceRoot: root, manifestVersion: manifest.version, glbFiles: records.filter(v => v.path.endsWith('.glb')).length, pngFiles: records.filter(v => v.path.endsWith('.png')).length, totalBytes: records.reduce((sum, v) => sum + v.bytes, 0), records };
fs.mkdirSync(path.join(workspace, '.reports'), { recursive: true });
const output = path.join(workspace, '.reports/model-package-audit.json');
fs.writeFileSync(output, JSON.stringify(report, null, 2));
console.log(JSON.stringify({ glbFiles: report.glbFiles, pngFiles: report.pngFiles, totalBytes: report.totalBytes, output }));
