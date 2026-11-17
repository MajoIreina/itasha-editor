import fs from 'node:fs';
import path from 'node:path';

const file = process.argv[2] ?? 'L:/SU7/3D/Blender/software/body.glb';
const nodeIndex = Number(process.argv[3] ?? 175);
const bytes = fs.readFileSync(file);
if (bytes.readUInt32LE(0) !== 0x46546c67 || bytes.readUInt32LE(4) !== 2) throw new Error('Invalid GLB');
const jsonLength = bytes.readUInt32LE(12);
const doc = JSON.parse(bytes.subarray(20, 20 + jsonLength));
const binStart = 20 + jsonLength + 8;
const component = { 5121: Uint8Array, 5123: Uint16Array, 5125: Uint32Array, 5126: Float32Array };
const width = { SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4 };
function accessor(index) {
  const a = doc.accessors[index];
  const view = doc.bufferViews[a.bufferView];
  const Ctor = component[a.componentType];
  const count = a.count * width[a.type];
  return new Ctor(bytes.buffer, bytes.byteOffset + binStart + (view.byteOffset ?? 0) + (a.byteOffset ?? 0), count);
}
const node = doc.nodes[nodeIndex];
const mesh = doc.meshes[node.mesh];
const primitive = mesh.primitives[0];
const positions = accessor(primitive.attributes.POSITION);
const indices = accessor(primitive.indices);
const normals = primitive.attributes.NORMAL === undefined ? null : accessor(primitive.attributes.NORMAL);
const vertices = positions.length / 3;
const parent = Array.from({ length: vertices }, (_, i) => i);
const find = (x) => {
  while (parent[x] !== x) {
    parent[x] = parent[parent[x]];
    x = parent[x];
  }
  return x;
};
const union = (a, b) => {
  a = find(a); b = find(b);
  if (a !== b) parent[b] = a;
};
for (let i = 0; i < indices.length; i += 3) {
  union(indices[i], indices[i + 1]);
  union(indices[i], indices[i + 2]);
}
const groups = new Map();
for (let face = 0; face < indices.length / 3; face++) {
  const root = find(indices[face * 3]);
  if (!groups.has(root)) groups.set(root, []);
  groups.get(root).push(face);
}
const bounds = (faces) => {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  let area = 0;
  let outward = 0;
  for (const face of faces) {
    const ids = [indices[face * 3], indices[face * 3 + 1], indices[face * 3 + 2]];
    const p = ids.map((id) => [positions[id * 3], positions[id * 3 + 1], positions[id * 3 + 2]]);
    for (const point of p) for (let axis = 0; axis < 3; axis++) {
      min[axis] = Math.min(min[axis], point[axis]);
      max[axis] = Math.max(max[axis], point[axis]);
    }
    const e = p[1].map((v, i) => v - p[0][i]);
    const f = p[2].map((v, i) => v - p[0][i]);
    const cross = [e[1] * f[2] - e[2] * f[1], e[2] * f[0] - e[0] * f[2], e[0] * f[1] - e[1] * f[0]];
    area += Math.hypot(...cross) * 0.5;
    const center = p[0].map((v, i) => (v + p[1][i] + p[2][i]) / 3);
    const length = Math.hypot(...cross);
    if (length > 1e-8 && cross[2] * center[2] + cross[0] * center[0] > 0) outward++;
  }
  return { faces: faces.length, areaM2: area, min, max, center: min.map((v, i) => (v + max[i]) / 2), outwardRatio: outward / Math.max(faces.length, 1) };
};
const components = [...groups.values()].map(bounds).sort((a, b) => b.faces - a.faces);
const result = { file, nodeIndex, nodeName: node.name, meshIndex: node.mesh, primitiveCount: mesh.primitives.length, vertices, triangles: indices.length / 3, components };
const output = process.argv[4] ?? '.reports/paint-node-175-components.json';
fs.mkdirSync(path.dirname(path.resolve(output)), { recursive: true });
fs.writeFileSync(output, JSON.stringify(result, null, 2));
console.log(JSON.stringify({ output, nodeName: node.name, triangles: result.triangles, components: components.length }));
