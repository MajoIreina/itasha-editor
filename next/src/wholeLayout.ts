import type { SurfaceInfo } from './canvasEngine';

export interface Tile<T extends SurfaceInfo> { surface: T; x: number; y: number }

/** Display positions only. Surface millimetres and the 3D mapping are unchanged. */
export function wholeLayout<T extends SurfaceInfo>(surfaces: T[]) {
  const gap = 180;
  const byId = new Map(surfaces.map(s => [s.id, s]));
  const tiles: Tile<T>[] = [];
  const used = new Set<string>();
  const get = (name: string) => byId.get('su7.' + name);
  const centerWidth = Math.max(1700, ...['hood', 'trunk', 'front-bumper', 'rear-bumper', 'rear-spoiler'].map(n => get(n)?.widthMm ?? 0));
  const sideWidth = Math.max(1000, ...['front-left-fender', 'front-right-fender', 'front-left-door', 'front-right-door', 'rear-left-door', 'rear-right-door'].map(n => get(n)?.widthMm ?? 0));
  const outerWidth = Math.max(0, ...['rear-left-fender', 'rear-right-fender', 'left-sill', 'right-sill'].map(n => get(n)?.widthMm ?? 0));
  const centerX = outerWidth + sideWidth + gap * 2;
  const put = (name: string, x: number, y: number) => {
    const surface = get(name);
    if (surface) { tiles.push({ surface, x, y }); used.add(surface.id); }
  };
  const central = (name: string, y: number) => put(name, centerX + (centerWidth - (get(name)?.widthMm ?? 0)) / 2, y);
  let y = 100;
  for (const name of ['front-c-ring', 'front-bumper']) {
    if (get(name)) { central(name, y); y += get(name)!.heightMm + gap; }
  }
  const hoodY = y;
  central('hood', hoodY);
  for (const side of ['left', 'right']) {
    const name = `front-${side}-fender`, s = get(name);
    put(name, side === 'left' ? centerX - gap - (s?.widthMm ?? 0) : centerX + centerWidth + gap, hoodY);
  }
  y += Math.max(...['hood', 'front-left-fender', 'front-right-fender'].map(n => get(n)?.heightMm ?? 0)) + gap;
  for (const side of ['left', 'right']) {
    const name = `front-${side}-lower-fender`, s = get(name);
    put(name, side === 'left' ? centerX - gap - (s?.widthMm ?? 0) : centerX + centerWidth + gap, y);
  }
  y += Math.max(0, ...['front-left-lower-fender', 'front-right-lower-fender'].map(n => get(n)?.heightMm ?? 0)) + gap;
  const cabinY = y;
  for (const row of ['front', 'rear']) {
    for (const side of ['left', 'right']) {
      const name = `${row}-${side}-door`, s = get(name);
      put(name, side === 'left' ? centerX - gap - (s?.widthMm ?? 0) : centerX + centerWidth + gap, y);
    }
    y += Math.max(0, ...['left', 'right'].map(side => get(`${row}-${side}-door`)?.heightMm ?? 0)) + gap;
  }
  // The one-piece quarter panel/A pillar runs alongside the cabin, not across the trunk row.
  for (const side of ['left', 'right']) {
    const name = `rear-${side}-fender`, s = get(name);
    put(name, side === 'left' ? outerWidth - (s?.widthMm ?? 0) : centerX + centerWidth + sideWidth + gap * 2, cabinY);
  }
  const cabinHeight = y - cabinY - gap;
  const flankBottom = Math.max(y, ...tiles.filter(t => t.surface.id.includes('rear-') && t.surface.id.includes('fender')).map(t => t.y + t.surface.heightMm + gap));
  y = flankBottom;
  for (const side of ['left', 'right']) {
    const name = `${side}-sill`, s = get(name);
    put(name, side === 'left' ? centerX - gap - (s?.widthMm ?? 0) : centerX + centerWidth + gap, y);
  }
  y += Math.max(0, ...['left-sill', 'right-sill'].map(n => get(n)?.heightMm ?? 0)) + gap;
  for (const name of ['trunk', 'rear-spoiler', 'rear-bumper', 'rear-c-ring']) {
    if (get(name)) { central(name, y); y += get(name)!.heightMm + gap; }
  }
  // Keep additional vehicle parts visible, rather than dropping unrecognised identifiers.
  for (const surface of surfaces.filter(s => !used.has(s.id))) {
    tiles.push({ surface, x: centerX + (centerWidth - surface.widthMm) / 2, y });
    y += surface.heightMm + gap;
  }
  return { tiles, reference: { x: centerX, y: cabinY, width: centerWidth, height: cabinHeight } };
}
