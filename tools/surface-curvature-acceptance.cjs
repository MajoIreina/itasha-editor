// Analytic curved meshes provide an independent reference for UV continuity and
// arc length. These tests do not depend on a browser or a particular car model.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
class Vector3 {
  constructor(x = 0, y = 0, z = 0) { Object.assign(this, { x, y, z }); }
  toArray() { return [this.x, this.y, this.z]; }
  clone() { return new Vector3(this.x, this.y, this.z); }
  add(v) { this.x += v.x; this.y += v.y; this.z += v.z; return this; }
  sub(v) { this.x -= v.x; this.y -= v.y; this.z -= v.z; return this; }
  addScaledVector(v, s) { this.x += v.x * s; this.y += v.y * s; this.z += v.z * s; return this; }
  multiplyScalar(s) { this.x *= s; this.y *= s; this.z *= s; return this; }
  dot(v) { return this.x * v.x + this.y * v.y + this.z * v.z; }
  cross(v) { const { x, y, z } = this; this.x = y * v.z - z * v.y; this.y = z * v.x - x * v.z; this.z = x * v.y - y * v.x; return this; }
  lengthSq() { return this.dot(this); }
  length() { return Math.sqrt(this.lengthSq()); }
  normalize() { return this.multiplyScalar(1 / (this.length() || 1)); }
  distanceTo(v) { return this.clone().sub(v).length(); }
  applyAxisAngle(axis, angle) { const source = this.clone(), c = Math.cos(angle), s = Math.sin(angle); return this.multiplyScalar(c).addScaledVector(axis.clone().cross(source), s).addScaledVector(axis, axis.dot(source) * (1 - c)); }
}
function cylinder(name, part, from, to, radius, zOffset = 0) {
  const columns = 48, rows = 12, faces = [];
  const point = (i, j) => { const a = from + (to - from) * i / columns; return new Vector3(radius * Math.sin(a), j / rows, radius * Math.cos(a) + zOffset); };
  for (let i = 0; i < columns; i++) for (let j = 0; j < rows; j++) {
    const a = point(i, j), b = point(i + 1, j), c = point(i + 1, j + 1), d = point(i, j + 1);
    for (const triangle of [[a, b, c], [a, c, d]]) {
      const center = triangle.reduce((v, p) => v.add(p), new Vector3()).multiplyScalar(1 / 3);
      faces.push({ part, region: Math.abs(center.x) > Math.abs(center.z - zOffset) ? center.x < 0 ? 'front' : 'rear' : 'left', points: triangle });
    }
  }
  return { name, userData: { studioFaces: faces } };
}
const root = path.resolve(__dirname, '..'), context = { window: {} };
vm.runInNewContext(fs.readFileSync(path.join(root, 'app/paint-body.js'), 'utf8'), context);
context.SU7Paint = context.window.SU7Paint;
vm.runInNewContext(fs.readFileSync(path.join(root, 'app/surface-wrap.js'), 'utf8'), context);
const SurfaceWrap = context.window.SU7SurfaceWrap, checks = [], measurements = [];
function check(name, value) { assert.ok(value, name); checks.push(name); console.log('PASS ' + name); }
for (const radius of [0.45, 0.8, 1.5]) {
  const left = cylinder('curved-door', 'doorLF', -1.05, 0, radius), right = cylinder('curved-body', 'body', 0, 1.05, radius);
  const inner = cylinder('interior-shell', 'body', -1.05, 1.05, radius, -0.18);
  const graph = new SurfaceWrap([left, right, inner], Vector3);
  const point = [0, 0.5, radius], start = left.userData.studioFaces.length - 12;
  const width = radius * 2.2;
  const wrap = graph.make({ mesh: left.name, face: start, point, normal: [0, 0, 1], right: [1, 0, 0], down: [0, 1, 0], width, height: 0.7, at: [0.5, 0.5] });
  check(`radius ${radius}: decal spans both curved components`, wrap.patches.some(p => p.mesh === left.name) && wrap.patches.some(p => p.mesh === right.name));
  check(`radius ${radius}: decal stays on the outer shell`, wrap.patches.every(p => p.mesh !== inner.name));
  const shared = new Map(); let maxUVError = 0, maxArcError = 0, sharedCount = 0;
  for (const patch of wrap.patches) {
    check(`radius ${radius}: ${patch.mesh}/${patch.region} retains full 3D geometry`, patch.xyz.length === patch.faces.length * 9 && patch.uv.length === patch.faces.length * 6);
    for (let i = 0; i < patch.xyz.length; i += 3) {
      const p = patch.xyz.slice(i, i + 3), uv = patch.uv.slice(i / 3 * 2, i / 3 * 2 + 2), key = p.map(n => n.toFixed(5)).join();
      assert.ok(uv.every(Number.isFinite));
      const old = shared.get(key); if (old) { sharedCount++; maxUVError = Math.max(maxUVError, Math.hypot(uv[0] - old[0], uv[1] - old[1])); } else shared.set(key, uv);
      const angle = Math.atan2(p[0], p[2]);
      if (Math.abs(angle) > 0.3 && Math.abs(angle) < 0.95) maxArcError = Math.max(maxArcError, Math.abs((uv[0] - 0.5) * width - radius * angle));
    }
  }
  check(`radius ${radius}: UVs remain continuous across seams and normal regions`, sharedCount > 100 && maxUVError < 1e-7);
  check(`radius ${radius}: curvature compensation follows analytic arc length`, maxArcError < radius * 0.005);
  check(`radius ${radius}: curved coverage spans multiple orthographic regions`, new Set(wrap.patches.map(p => p.region)).size > 1);
  const moved = graph.frame({ normal: [Math.sin(0.6), 0, Math.cos(0.6)] }, 'left', 0, { normal: [0, 0, 1], right: [1, 0, 0] });
  check(`radius ${radius}: tangent frame follows the new curved normal`, Math.abs(new Vector3(...moved.right).dot(new Vector3(...moved.normal))) < 1e-9 && Math.abs(new Vector3(...moved.right).length() - 1) < 1e-9);
  measurements.push({ radius, sharedCount, maxUVError, maxArcError });
}
const output = path.join(__dirname, 'acceptance', 'surface-curvature'); fs.mkdirSync(output, { recursive: true });
fs.writeFileSync(path.join(output, 'verification.json'), JSON.stringify({ time: new Date().toISOString(), checks, measurements }, null, 2));
console.log(`DONE ${checks.length}`);
