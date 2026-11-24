import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { THEME_KEY, applyTheme, readThemeChoice, resolveTheme, setThemeChoice } from './theme';

describe('theme', () => {
  beforeEach(() => {
    window.localStorage.clear();
    document.documentElement.removeAttribute('data-theme');
  });
  afterEach(() => {
    delete window.NimragApp;
  });

  it('is light unless the stored choice says otherwise', () => {
    expect(readThemeChoice()).toBe('light');
    window.localStorage.setItem(THEME_KEY, 'dark');
    expect(readThemeChoice()).toBe('dark');
    window.localStorage.setItem(THEME_KEY, 'system');
    expect(readThemeChoice()).toBe('system');
    window.localStorage.setItem(THEME_KEY, 'neon');
    expect(readThemeChoice()).toBe('light');
  });

  it('resolves "system" from the system setting and forces dark inside the Android app', () => {
    expect(resolveTheme('light', true, false)).toBe('light');
    expect(resolveTheme('dark', false, false)).toBe('dark');
    expect(resolveTheme('system', true, false)).toBe('dark');
    expect(resolveTheme('system', false, false)).toBe('light');
    expect(resolveTheme('light', false, true)).toBe('dark');
  });

  it('sets data-theme from the stored choice and stores a new one', () => {
    expect(applyTheme()).toBe('light');
    expect(document.documentElement.dataset['theme']).toBe('light');
    setThemeChoice('dark');
    expect(window.localStorage.getItem(THEME_KEY)).toBe('dark');
    expect(document.documentElement.dataset['theme']).toBe('dark');
    window.NimragApp = { native: true };
    setThemeChoice('light');
    expect(document.documentElement.dataset['theme']).toBe('dark');
  });
});
