export type Rgb = [number, number, number];
export type Hsv = { h: number; s: number; v: number };
export const clamp = (n: number, min: number, max: number) => Math.min(max, Math.max(min, n));

export function parseHex(input: string): string | null {
  let text = input.trim().replace(/^#/, '');
  if (/^[\da-f]{3}$/i.test(text)) text = [...text].map(c => c + c).join('');
  return /^[\da-f]{6}$/i.test(text) ? '#' + text.toLowerCase() : null;
}
export function hexToRgb(hex: string): Rgb {
  const text = parseHex(hex);
  if (!text) throw new Error('Invalid RGB color');
  return [1, 3, 5].map(i => parseInt(text.slice(i, i + 2), 16)) as Rgb;
}
export function rgbToHex(rgb: Rgb): string {
  return '#' + rgb.map(n => Math.round(clamp(n, 0, 255)).toString(16).padStart(2, '0')).join('');
}
export function rgbToHsv([r0, g0, b0]: Rgb, fallbackHue = 0): Hsv {
  const [r, g, b] = [r0, g0, b0].map(n => n / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b), delta = max - min;
  let h = fallbackHue;
  if (delta) {
    h = max === r ? (g - b) / delta : max === g ? (b - r) / delta + 2 : (r - g) / delta + 4;
    h = (h * 60 + 360) % 360;
  }
  return { h, s: max ? delta / max : 0, v: max };
}
export function hsvToRgb({ h, s, v }: Hsv): Rgb {
  const hue = ((h % 360) + 360) % 360 / 60;
  const c = clamp(v, 0, 1) * clamp(s, 0, 1), x = c * (1 - Math.abs(hue % 2 - 1)), m = clamp(v, 0, 1) - c;
  const channels = hue < 1 ? [c, x, 0] : hue < 2 ? [x, c, 0] : hue < 3 ? [0, c, x] : hue < 4 ? [0, x, c] : hue < 5 ? [x, 0, c] : [c, 0, x];
  return channels.map(n => Math.round((n + m) * 255)) as Rgb;
}
export function parseRgbChannel(value: string): number | null {
  if (!/^\d{1,3}$/.test(value.trim())) return null;
  const n = Number(value);
  return n <= 255 ? n : null;
}
