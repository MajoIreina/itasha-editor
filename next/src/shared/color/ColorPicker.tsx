import { useEffect, useRef, useState } from 'react';
import type { KeyboardEvent, PointerEvent } from 'react';
import { clamp, hexToRgb, hsvToRgb, parseHex, parseRgbChannel, rgbToHex, rgbToHsv } from './colorMath';
import type { Hsv, Rgb } from './colorMath';
import './colorPicker.css';

/** A transaction per drag/input edit, so selecting a colour creates one undo step. */
export function ColorPicker({ label, value, disabled = false, onChange }: {
  label: string; value: string; disabled?: boolean; onChange: (hex: string) => void;
}) {
  const [hsv, setHsv] = useState(() => rgbToHsv(hexToRgb(value)));
  const [hex, setHex] = useState(value);
  const [rgb, setRgb] = useState(() => hexToRgb(value).map(String));
  const [error, setError] = useState('');
  const live = useRef(hsv), active = useRef(false), previous = useRef(hsv);
  const setDraft = (next: Hsv) => {
    live.current = next; setHsv(next);
    const channels = hsvToRgb(next);
    setRgb(channels.map(String)); setHex(rgbToHex(channels)); setError('');
  };
  useEffect(() => {
    active.current = false;
    setDraft(rgbToHsv(hexToRgb(value), live.current.h));
  }, [value]);
  const commit = () => {
    const next = rgbToHex(hsvToRgb(live.current));
    if (!disabled && next !== parseHex(value)) onChange(next);
  };
  const cancel = () => { active.current = false; setDraft(previous.current); };
  const point = (e: PointerEvent<HTMLDivElement>, axis: 'sv' | 'h') => {
    const r = e.currentTarget.getBoundingClientRect();
    const x = clamp((e.clientX - r.left) / r.width, 0, 1), y = clamp((e.clientY - r.top) / r.height, 0, 1);
    setDraft(axis === 'h' ? { ...live.current, h: y * 360 } : { ...live.current, s: x, v: 1 - y });
  };
  const pointerHandlers = (axis: 'sv' | 'h') => ({
    onPointerDown: (e: PointerEvent<HTMLDivElement>) => {
      if (disabled || e.button !== 0) return;
      previous.current = live.current; active.current = true;
      e.currentTarget.focus(); e.currentTarget.setPointerCapture(e.pointerId); point(e, axis);
    },
    onPointerMove: (e: PointerEvent<HTMLDivElement>) => { if (active.current && e.currentTarget.hasPointerCapture(e.pointerId)) point(e, axis); },
    onPointerUp: (e: PointerEvent<HTMLDivElement>) => {
      if (!active.current || !e.currentTarget.hasPointerCapture(e.pointerId)) return;
      point(e, axis); active.current = false; e.currentTarget.releasePointerCapture(e.pointerId); commit();
    },
    onPointerCancel: cancel,
    onLostPointerCapture: () => { if (active.current) cancel(); },
  });
  const keyboard = (e: KeyboardEvent<HTMLDivElement>, axis: 'sv' | 'h') => {
    if (disabled) return;
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); active.current = false; setDraft(rgbToHsv(hexToRgb(value), live.current.h)); return; }
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(e.key)) return;
    e.preventDefault();
    const step = e.shiftKey ? 10 : 1;
    if (axis === 'h') {
      const h = e.key === 'Home' ? 0 : e.key === 'End' ? 360 : clamp(live.current.h + (['ArrowUp', 'ArrowLeft'].includes(e.key) ? -step : step), 0, 360);
      setDraft({ ...live.current, h });
    } else {
      const { s, v } = live.current;
      setDraft({ ...live.current,
        s: e.key === 'Home' ? 0 : e.key === 'End' ? 1 : clamp(s + (e.key === 'ArrowLeft' ? -step / 100 : e.key === 'ArrowRight' ? step / 100 : 0), 0, 1),
        v: clamp(v + (e.key === 'ArrowUp' ? step / 100 : e.key === 'ArrowDown' ? -step / 100 : 0), 0, 1) });
    }
  };
  const keyup = (e: KeyboardEvent<HTMLDivElement>) => { if (['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(e.key)) commit(); };
  const fieldKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') e.currentTarget.blur();
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); setDraft(rgbToHsv(hexToRgb(value), live.current.h)); }
  };
  const acceptHex = () => {
    const parsed = parseHex(hex);
    if (!parsed) { setError('HEX 请输入 3 位或 6 位十六进制颜色。'); return; }
    setDraft(rgbToHsv(hexToRgb(parsed), live.current.h)); commit();
  };
  const acceptRgb = () => {
    const values = rgb.map(parseRgbChannel);
    if (values.some(n => n === null)) { setError('R、G、B 必须为 0–255 的整数。'); return; }
    setDraft(rgbToHsv(values as Rgb, live.current.h)); commit();
  };
  const draftColor = rgbToHex(hsvToRgb(hsv));
  return <fieldset className="color-picker" disabled={disabled} aria-label={label}>
    <legend>{label}</legend>
    <div className="color-plane-row">
      <div className="color-swatches" aria-hidden="true"><span style={{ background: draftColor }} /><span style={{ background: value }} /></div>
      <div className="color-sv" role="slider" tabIndex={disabled ? -1 : 0} aria-disabled={disabled} aria-label={label + '饱和度和明度'} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(hsv.s * 100)} aria-valuetext={`饱和度 ${Math.round(hsv.s * 100)}%，明度 ${Math.round(hsv.v * 100)}%；左右调整饱和度，上下调整明度`}
        style={{ backgroundColor: `hsl(${hsv.h} 100% 50%)` }} {...pointerHandlers('sv')} onKeyDown={e => keyboard(e, 'sv')} onKeyUp={keyup} onBlur={() => { if (!active.current) commit(); }}>
        <span className="color-sv-cursor" style={{ left: `${hsv.s * 100}%`, top: `${(1 - hsv.v) * 100}%` }} />
      </div>
      <div className="color-hue" role="slider" tabIndex={disabled ? -1 : 0} aria-disabled={disabled} aria-label={label + '色相'} aria-orientation="vertical" aria-valuemin={0} aria-valuemax={360} aria-valuenow={Math.round(hsv.h)}
        {...pointerHandlers('h')} onKeyDown={e => keyboard(e, 'h')} onKeyUp={keyup} onBlur={() => { if (!active.current) commit(); }}>
        <span style={{ top: `${hsv.h / 360 * 100}%` }} />
      </div>
    </div>
    <div className="color-rgb-fields">{['R', 'G', 'B'].map((channel, i) => <label key={channel}>{channel}<input aria-label={label + ' ' + channel} inputMode="numeric" maxLength={3} value={rgb[i]} aria-invalid={parseRgbChannel(rgb[i]) === null} onChange={e => { const next = [...rgb]; next[i] = e.target.value; setRgb(next); setError(''); }} onBlur={acceptRgb} onKeyDown={fieldKey} /></label>)}</div>
    <label className="color-hex-field">HEX<input aria-label={label + ' HEX'} spellCheck={false} maxLength={7} value={hex} aria-invalid={parseHex(hex) === null} onChange={e => { setHex(e.target.value); setError(''); }} onBlur={acceptHex} onKeyDown={fieldKey} /></label>
    {error && <small className="property-error" role="alert">{error}</small>}
  </fieldset>;
}
