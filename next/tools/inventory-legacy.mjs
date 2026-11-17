import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
function safe(relative) {
  const target = path.resolve(root, relative);
  if (!target.startsWith(root + path.sep)) throw Error('Asset path escapes repository');
  const actual = fs.realpathSync(target);
  if (!actual.startsWith(fs.realpathSync(root) + path.sep)) throw Error('Asset symlink escapes repository');
  return actual;
}
const read = relative => JSON.parse(fs.readFileSync(safe(relative), 'utf8'));
const seen = new Map();
const vehicles = [];
for (const vehicle of read('config/vehicles.json').vehicles) {
  const entries = [];
  const manifests = [vehicle.manifest, vehicle.extras].filter(Boolean);
  for (const manifest of manifests) {
    const json = read(manifest);
    for (const entry of Array.isArray(json) ? json : json.entries) {
      if (!entry.file) continue;
      const relative = path.relative(root, safe(path.join(vehicle.assetRoot, entry.file))).replaceAll('\\', '/');
      const urlPath = new URL(entry.url).pathname;
      entries.push({ file: relative, sourceExtension: path.extname(urlPath).toLowerCase() || '(none)' });
      if (seen.has(relative)) continue;
      const file = safe(relative);
      const bytes = fs.statSync(file).size;
      const handle = fs.openSync(file, 'r');
      const head = Buffer.alloc(16);
      const length = fs.readSync(handle, head, 0, 16, 0);
      fs.closeSync(handle);
      const signature = head.subarray(0, length);
      let format = 'unknown';
      if (signature.subarray(0, 4).toString() === 'glTF') format = 'glb';
      else if (signature.subarray(0, 4).equals(Buffer.from([0,97,115,109]))) format = 'wasm';
      else if (signature.subarray(0, 4).equals(Buffer.from([137,80,78,71]))) format = 'png';
      else if (signature[0] === 255 && signature[1] === 216) format = 'jpeg';
      else if (signature.subarray(0, 4).equals(Buffer.from([171,75,84,88]))) format = 'ktx';
      const digest = crypto.createHash('sha256');
      for await (const chunk of fs.createReadStream(file)) digest.update(chunk);
      seen.set(relative, { file: relative, bytes, sha256: digest.digest('hex'), format, headerHex: signature.toString('hex') });
    }
  }
  vehicles.push({
    id: vehicle.id, entries: entries.length,
    uniqueFiles: new Set(entries.map(e => e.file)).size,
    candidateModels: entries.filter(e => ['.mi','.glb','.gltf','.bin'].includes(e.sourceExtension)),
    extensions: Object.fromEntries([...new Set(entries.map(e => e.sourceExtension))].sort().map(ext => [ext, entries.filter(e => e.sourceExtension === ext).length])),
  });
}
const files = [...seen.values()];
const report = {
  generatedAt: new Date().toISOString(), scope: 'Read-only manifest inventory; no engine execution or decryption',
  vehicles, uniqueFiles: files.length, totalBytes: files.reduce((n,f) => n + f.bytes, 0),
  formats: Object.fromEntries([...new Set(files.map(f => f.format))].map(format => [format, files.filter(f => f.format === format).length])),
  files,
};
const output = path.join(root, 'next/.reports');
fs.mkdirSync(output, { recursive: true });
fs.writeFileSync(path.join(output, 'legacy-assets.json'), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ vehicles: vehicles.map(({ id, entries, uniqueFiles, candidateModels }) => ({ id, entries, uniqueFiles, candidateModelReferences: candidateModels.length })), uniqueFiles: report.uniqueFiles, totalBytes: report.totalBytes, formats: report.formats }, null, 2));
