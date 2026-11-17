// Build a minimal data-only catalog. No captured HTML, executable code or remote URLs.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const registry = JSON.parse(fs.readFileSync(path.join(root, 'config/vehicles.json')));
const result = [];
for (const vehicle of registry.vehicles) {
  const bytes = fs.readFileSync(path.join(root, vehicle.adapter));
  const car = JSON.parse(bytes).configuration.data.carSsuDetail;
  const groups = { paint: [], wheels: [], interior: [], steering: [] };
  const defaults = {};
  function walk(item) {
    if (!item || typeof item !== 'object') return;
    const key = item.itemIdentifier === '1_WG' ? 'paint' : item.itemIdentifier === '1_LG' ? 'wheels' : item.itemIdentifier === '1_NSYS' ? 'interior' : (item.itemIdentifier || '').toLowerCase().includes('steerwheel') ? 'steering' : null;
    if (key && item.ssuId && item.name && !groups[key].some(o => o.id === String(item.ssuId))) {
      const color = item.theme?.middleColors?.[0]?.value;
      groups[key].push({
        id: String(item.ssuId), name: item.name,
        swatch: /^rgba?\([\d.,\s]+\)$/.test(color || '') ? color : null,
        // Preserve source purchase restrictions for reference, not local design validation.
        excludes: [...new Set(Object.values(item.mutexMap || {}).flat().map(String))],
      });
      if (item.defaultSelect && !defaults[key]) defaults[key] = String(item.ssuId);
    }
    for (const child of Object.values(item)) if (child && typeof child === 'object') walk(child);
  }
  walk(car.configurator);
  for (const key of Object.keys(groups)) {
    if (!groups[key].length) groups[key].push({ id: 'factory', name: '保留原车配置', swatch: null, excludes: [] });
    defaults[key] ||= groups[key][0].id;
    if (groups[key].some(o => o.id === 'factory')) continue;
    if (!groups[key].some(o => o.id === defaults[key])) throw Error('Invalid default');
  }
  result.push({
    id: vehicle.id, name: car.name || vehicle.label,
    family: vehicle.id.startsWith('su7') ? '轿车' : vehicle.id.startsWith('yu7') ? 'SUV' : '多功能车型',
    bodyStyle: vehicle.id.startsWith('su7') ? 'sedan' : vehicle.id.startsWith('yu7') ? 'suv' : 'mpv',
    sourceSha256: crypto.createHash('sha256').update(bytes).digest('hex'),
    ...groups,
    defaults: { catalogVersion: 1, ...defaults },
  });
}
const out = path.join(root, 'next/catalog');
fs.mkdirSync(out, { recursive: true });
fs.writeFileSync(path.join(out, 'vehicles.json'), JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result.map(v => ({ id: v.id, name: v.name, options: Object.fromEntries(['paint','wheels','interior','steering'].map(k => [k, v[k].length])) })), null, 2));
