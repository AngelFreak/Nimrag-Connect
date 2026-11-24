import { useSyncExternalStore } from 'react';
import { isNativeApp } from '../native';

// The web theme: light by default, dark on request, or whatever the system prefers. The choice is
// kept in localStorage; `<html data-theme>` always holds the resolved theme ('light' | 'dark'),
// which styles/tokens.css keys off. The inline script in index.html does the same before first
// paint, so keep both in step. The Android app always gets dark: it has no theme setting.

export type ThemeChoice = 'light' | 'dark' | 'system';
export type Theme = 'light' | 'dark';

export const THEME_KEY = 'nimrag.theme';
export const THEME_CHANGE_EVENT = 'nimrag:theme';

/** Address-bar colour per theme (the `theme-color` meta tag). */
const THEME_COLOR: Record<Theme, string> = { light: '#ffffff', dark: '#0F1417' };

export function readThemeChoice(): ThemeChoice {
  try {
    const stored = window.localStorage.getItem(THEME_KEY);
    return stored === 'dark' || stored === 'system' ? stored : 'light';
  } catch {
    return 'light';
  }
}

/** The theme a choice resolves to; the native app is dark whatever was chosen. */
export function resolveTheme(choice: ThemeChoice, prefersDark: boolean, native = isNativeApp()): Theme {
  if (native) return 'dark';
  return choice === 'system' ? (prefersDark ? 'dark' : 'light') : choice;
}

const prefersDark = (): boolean => typeof window.matchMedia === 'function' && window.matchMedia('(prefers-color-scheme: dark)').matches;

/** Sets `data-theme` (and the address-bar colour) from the stored choice. */
export function applyTheme(): Theme {
  const theme = resolveTheme(readThemeChoice(), prefersDark());
  const root = document.documentElement;
  if (root.dataset['theme'] !== theme) {
    root.dataset['theme'] = theme;
    window.dispatchEvent(new Event(THEME_CHANGE_EVENT));
  }
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', THEME_COLOR[theme]);
  return theme;
}

export function setThemeChoice(choice: ThemeChoice): void {
  try {
    window.localStorage.setItem(THEME_KEY, choice);
  } catch {
    // Storage blocked: the choice lasts until the page closes.
  }
  applyTheme();
  window.dispatchEvent(new Event(THEME_CHANGE_EVENT));
}

/** Applies the theme now and follows the system setting while the choice is "system". */
export function initTheme(): void {
  applyTheme();
  if (typeof window.matchMedia !== 'function') return;
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    if (readThemeChoice() === 'system') applyTheme();
  });
}

function subscribe(onChange: () => void): () => void {
  window.addEventListener(THEME_CHANGE_EVENT, onChange);
  return () => window.removeEventListener(THEME_CHANGE_EVENT, onChange);
}

/** The stored choice, live. */
export function useThemeChoice(): ThemeChoice {
  return useSyncExternalStore(subscribe, readThemeChoice, () => 'light');
}

/** The resolved theme, live (charts redraw when it changes). */
export function useTheme(): Theme {
  return useSyncExternalStore(
    subscribe,
    () => (document.documentElement.dataset['theme'] === 'dark' ? 'dark' : 'light'),
    () => 'light',
  );
}
