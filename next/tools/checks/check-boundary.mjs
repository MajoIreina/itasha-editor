import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const forbidden = [
  [/<iframe\b/i, 'iframe'],
  [/\bfetch\s*\(|\bXMLHttpRequest\b|\bWebSocket\s*\(/, 'browser network client'],
  [/serviceWorker|navigator\.serviceWorker/, 'service worker'],
  [/https?:\/\/(?:localhost|127\.0\.0\.1):(?:8765|8000|3000)/, 'legacy HTTP backend'],
  [/resources[\\/](?:data|custom)|backend-adapter/, 'legacy engine dependency'],
];
const errors = [];
function walk(dir) {
  for (const name of fs.readdirSync(dir)) {
    const file = path.join(dir, name);
    if (fs.statSync(file).isDirectory()) walk(file);
    else if (/\.(ts|tsx|rs|css)$/.test(file)) {
      const text = fs.readFileSync(file, 'utf8');
      for (const [pattern, label] of forbidden) if (pattern.test(text)) errors.push(path.relative(root, file) + ': ' + label);
    }
  }
}
for (const dir of ['src', 'crates', 'src-tauri/src']) walk(path.join(root, dir));
if (errors.length) { console.error(errors.join('\n')); process.exitCode = 1; }
else console.log('Application source boundary passed: no iframe, browser HTTP client, service worker or legacy engine import. Static check only; runtime checks remain required.');
