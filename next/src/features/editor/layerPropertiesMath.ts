import type { Placement } from '../../bridge/desktop';

/** Resize about the layer centre. A ratio lock must respect both dimension limits. */
export function resizeProperty(p: Placement, key: 'widthMm' | 'heightMm', value: number): Placement | null {
  if (!Number.isFinite(value) || value < 1 || value > 10000) return null;
  const next = { ...p, [key]: value };
  if (p.aspectRatioLocked) {
    const other = key === 'widthMm' ? 'heightMm' : 'widthMm';
    next[other] = p[other] * value / p[key];
  }
  return [next.widthMm, next.heightMm].every(n => Number.isFinite(n) && n >= 1 && n <= 10000) ? next : null;
}
