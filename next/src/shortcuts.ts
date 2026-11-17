export function shortcutFromEvent(event: Pick<KeyboardEvent, 'ctrlKey' | 'altKey' | 'metaKey' | 'shiftKey' | 'code'> & { isComposing?: boolean }): string | null {
  if (!event.ctrlKey || event.altKey || event.metaKey || event.isComposing) return null;
  const key = event.code === 'Comma' ? 'Comma' : /^Key[A-Z]$/.test(event.code) ? event.code.slice(3) : '';
  return key ? 'Ctrl+' + (event.shiftKey ? 'Shift+' : '') + key : null;
}
export const displayShortcut = (value: string) => value.replace('Comma', ',');
