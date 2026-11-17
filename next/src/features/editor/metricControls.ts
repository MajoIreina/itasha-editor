import type { Layer } from '../../bridge/desktop';
import type { CoverageFragment } from './canvasEngine';
import { HANDLES, transformLayer } from './transform.ts';

export interface Point { x: number; y: number }
/** Columns mapping unrotated physical image millimetres into the local paint chart. */
export interface MetricFrame { x: Point; y: Point }
export const IDENTITY_FRAME: MetricFrame = { x: { x: 1, y: 0 }, y: { x: 0, y: 1 } };
const finite = (point: Point) => Number.isFinite(point.x) && Number.isFinite(point.y);
const determinant = (a: Point, b: Point) => a.x * b.y - a.y * b.x;
const subtract = (a: Point, b: Point): Point => ({ x: a.x - b.x, y: a.y - b.y });
const vector = ([x, y]: [number, number]): Point => ({ x, y });

export function chartVector(frame: MetricFrame, metric: Point): Point {
  return { x: frame.x.x * metric.x + frame.y.x * metric.y, y: frame.x.y * metric.x + frame.y.y * metric.y };
}
export function metricVector(frame: MetricFrame, chart: Point): Point | null {
  const det = determinant(frame.x, frame.y);
  if (!finite(frame.x) || !finite(frame.y) || !finite(chart)
    || Math.abs(det) <= Math.hypot(frame.x.x, frame.x.y) * Math.hypot(frame.y.x, frame.y.y) * 1e-10) return null;
  const result = { x: determinant(chart, frame.y) / det, y: determinant(frame.x, chart) / det };
  return finite(result) ? result : null;
}

/** Infer the anchor's local metric from a source fragment containing the image centre.
 * imageUv is before flipX; texture reflection must not reverse handle identity. */
export function coverageMetricFrame(layer: Layer, fragments: CoverageFragment[]): MetricFrame | null {
  const p = layer.placement;
  if (![p.xMm, p.yMm, p.widthMm, p.heightMm, p.rotationDeg].every(Number.isFinite)
    || p.widthMm <= 0 || p.heightMm <= 0) return null;
  let best: { area: number; frame: MetricFrame } | null = null;
  for (const fragment of fragments) {
    if (fragment.partId !== p.surfaceId || fragment.paintUv.length !== 3 || fragment.imageUv.length !== 3) continue;
    const image = fragment.imageUv.map(vector), paint = fragment.paintUv.map(vector);
    if (![...image, ...paint].every(finite)) continue;
    const a = subtract(image[1], image[0]), b = subtract(image[2], image[0]);
    const det = determinant(a, b);
    if (Math.abs(det) <= 1e-12) continue;
    const q = subtract({ x: 0.5, y: 0.5 }, image[0]);
    const v = determinant(q, b) / det, w = determinant(a, q) / det;
    if (Math.min(1 - v - w, v, w) < -1e-5) continue;
    const pa = subtract(paint[1], paint[0]), pb = subtract(paint[2], paint[0]);
    const imageX = { x: (pa.x * b.y - pb.x * a.y) / det / p.widthMm, y: (pa.y * b.y - pb.y * a.y) / det / p.widthMm };
    const imageY = { x: (pb.x * a.x - pa.x * b.x) / det / p.heightMm, y: (pb.y * a.x - pa.y * b.x) / det / p.heightMm };
    const angle = p.rotationDeg * Math.PI / 180, c = Math.cos(angle), s = Math.sin(angle);
    const frame = {
      x: { x: imageX.x * c - imageY.x * s, y: imageX.y * c - imageY.y * s },
      y: { x: imageX.x * s + imageY.x * c, y: imageX.y * s + imageY.y * c },
    };
    const center = { x: paint[0].x + pa.x * v + pb.x * w, y: paint[0].y + pa.y * v + pb.y * w };
    const tolerance = Math.max(0.01, (Math.hypot(imageX.x, imageX.y) * p.widthMm + Math.hypot(imageY.x, imageY.y) * p.heightMm) * 1e-5);
    if (!metricVector(frame, { x: 0, y: 0 }) || Math.hypot(center.x - p.xMm, center.y - p.yMm) > tolerance) continue;
    if (!best || Math.abs(det) > best.area) best = { area: Math.abs(det), frame };
  }
  return best?.frame ?? null;
}

export function controlPoints(layer: Layer, frame: MetricFrame, rotationDistance: number): Point[] {
  const p = layer.placement, angle = p.rotationDeg * Math.PI / 180, c = Math.cos(angle), s = Math.sin(angle);
  const at = (x: number, y: number) => {
    const d = chartVector(frame, { x: c * x - s * y, y: s * x + c * y });
    return { x: p.xMm + d.x, y: p.yMm + d.y };
  };
  const points = HANDLES.map(([x, y]) => at(x * p.widthMm / 2, y * p.heightMm / 2));
  const outward = chartVector(frame, { x: s, y: -c }), length = Math.hypot(outward.x, outward.y);
  points.push({ x: points[1].x + outward.x / length * rotationDistance, y: points[1].y + outward.y / length * rotationDistance });
  return points;
}

export function nearestControl(points: Point[], pointer: Point, radius: number): number | null {
  let selected: number | null = null, distance = radius;
  points.forEach((point, index) => {
    const next = Math.hypot(point.x - pointer.x, point.y - pointer.y);
    if (next < distance) { distance = next; selected = index; }
  });
  return selected;
}

/** Match Rust's physical resize/rotation, then put its centre shift back in chart units. */
export function transformMetricLayer(layer: Layer, frame: MetricFrame, start: Point, current: Point, handle: number | null, rotate: boolean, shift: boolean): Layer | null {
  const p = layer.placement, center = { x: p.xMm, y: p.yMm };
  const a = metricVector(frame, subtract(start, center)), b = metricVector(frame, subtract(current, center));
  if (!a || !b || (handle !== null && (!Number.isInteger(handle) || handle < 0 || handle >= HANDLES.length))) return null;
  if (rotate && (Math.hypot(a.x, a.y) < 1e-6 || Math.hypot(b.x, b.y) < 1e-6)) return null;
  const transformed = transformLayer({ ...layer, placement: { ...p, xMm: 0, yMm: 0 } }, a, b, handle, rotate, shift);
  const next = transformed.placement, delta = chartVector(frame, { x: next.xMm, y: next.yMm });
  const placement = { ...next, xMm: p.xMm + delta.x, yMm: p.yMm + delta.y };
  if (![placement.xMm, placement.yMm, placement.widthMm, placement.heightMm, placement.rotationDeg].every(Number.isFinite)
    || placement.widthMm < 1 || placement.widthMm > 10000 || placement.heightMm < 1 || placement.heightMm > 10000) return null;
  return { ...layer, placement };
}
