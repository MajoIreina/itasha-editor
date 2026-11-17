import { useLayoutEffect } from 'react';
import type { Settings } from './bridge';

export function useTheme(preference: Settings['theme']) {
  useLayoutEffect(() => {
    const system = window.matchMedia('(prefers-color-scheme: dark)');
    const apply = () => {
      document.documentElement.dataset.theme = preference === 'system'
        ? (system.matches ? 'dark' : 'light') : preference;
    };
    apply();
    if (preference === 'system') system.addEventListener('change', apply);
    return () => system.removeEventListener('change', apply);
  }, [preference]);
}
