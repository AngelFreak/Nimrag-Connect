// The accessibility checks of real.access.spec.ts. The `*InPage` functions run inside the browser
// (Playwright serialises them), so each one is self-contained: no imports, no shared helpers.

import AxeBuilder from '@axe-core/playwright';
import type { Page } from '@playwright/test';
import { AXE_TAGS } from './helpers';

// ---------------------------------------------------------------- facts about the page

export interface PageFacts {
  title: string;
  /** Text of every visible level-1 heading. */
  h1: string[];
  /** Landmarks: how many `main` regions the page has. */
  mains: number;
  lang: string;
}

export function pageFactsInPage(): PageFacts {
  const visible = (el: Element) => (el as HTMLElement).checkVisibility({ checkVisibilityCSS: true }) && !el.closest('[aria-hidden="true"], [inert]');
  const h1 = [...document.querySelectorAll('h1, [role="heading"][aria-level="1"]')].filter(visible).map((h) => (h.textContent ?? '').replace(/\s+/g, ' ').trim());
  return { title: document.title, h1, mains: document.querySelectorAll('main, [role="main"]').length, lang: document.documentElement.lang };
}

// ---------------------------------------------------------------- controls and keyboard

export interface Control {
  id: number;
  /** tag.class "text" of the element, short enough to find it. */
  label: string;
  /** Browser puts it in the Tab order. */
  tabbable: boolean;
  /** Radio buttons of one name and the members of a composite widget share a Tab stop. */
  group: string | null;
  /** Set when it is a control that cannot be reached with the keyboard. */
  problem: string | null;
}

/** Every visible control of the page, with an id that `focusInPage` reports back. */
export function controlsInPage(): Control[] {
  const w = window as unknown as { __a11y?: { ids: WeakMap<Element, number>; next: number } };
  w.__a11y ??= { ids: new WeakMap(), next: 1 };
  const reg = w.__a11y;
  const idOf = (el: Element) => {
    let id = reg.ids.get(el);
    if (!id) {
      id = reg.next++;
      reg.ids.set(el, id);
    }
    return id;
  };
  const describe = (el: Element) => {
    const cls = typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\s+/).slice(0, 2).join('.') : '';
    const text = (el.getAttribute('aria-label') ?? el.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 40);
    return `${el.tagName.toLowerCase()}${cls}${text ? ` "${text}"` : ''}`;
  };
  const native = 'a[href], area[href], button, input:not([type="hidden"]), select, textarea, summary, audio[controls], video[controls], [contenteditable=""], [contenteditable="true"]';
  const roles = ['button', 'link', 'tab', 'checkbox', 'radio', 'switch', 'menuitem', 'option', 'slider', 'textbox', 'combobox'].map((r) => `[role="${r}"]`).join(', ');
  const composite = '[role="tablist"], [role="radiogroup"], [role="listbox"], [role="menu"], [role="menubar"], [role="toolbar"], [role="grid"], [role="tree"]';

  const out: Control[] = [];
  for (const el of document.querySelectorAll<HTMLElement>(`${native}, ${roles}, [tabindex]:not([tabindex="-1"])`)) {
    if (!el.checkVisibility({ checkVisibilityCSS: true })) continue;
    if (el.closest('[inert]')) continue;
    if ((el as HTMLButtonElement).disabled) continue;
    let group: string | null = null;
    if (el instanceof HTMLInputElement && el.type === 'radio') group = `radio:${el.name || idOf(el)}`;
    else {
      const widget = el.closest(composite);
      if (widget) group = `widget:${idOf(widget)}`;
      // A labelled group whose members are out of the Tab order except one (the calendar's days) is a roving tabindex.
      else if (el.tabIndex < 0) {
        const roving = el.closest('[role="group"][aria-label]');
        if (roving && [...roving.querySelectorAll<HTMLElement>(native)].some((o) => o.tabIndex >= 0)) group = `widget:${idOf(roving)}`;
      }
      else if (el.closest('[role="group"][aria-label]')) {
        const roving = el.closest('[role="group"][aria-label]');
        if (roving && [...roving.querySelectorAll<HTMLElement>(native)].some((o) => o.tabIndex < 0 && o.tagName === el.tagName)) group = `widget:${idOf(roving)}`;
      }
    }
    const tabbable = el.tabIndex >= 0;
    out.push({
      id: idOf(el),
      label: describe(el),
      tabbable,
      group,
      problem: !tabbable && group === null ? 'tabindex="-1" takes it out of the Tab order and nothing else gets to it' : null,
    });
  }
  return out;
}

/** Things that react to the mouse but cannot be focused: a clickable `div`, a `span` that looks like a link. */
export function mouseOnlyInPage(): string[] {
  const focusable =
    'a[href], area[href], button, input:not([type="hidden"]), select, textarea, summary, label, [tabindex], [role="button"], [role="link"], [role="tab"], [role="checkbox"], [role="radio"], [role="switch"], [role="menuitem"], [role="option"]';
  const out: string[] = [];
  for (const el of document.body.querySelectorAll<HTMLElement>('*')) {
    if (!el.checkVisibility({ checkVisibilityCSS: true })) continue;
    if (getComputedStyle(el).cursor !== 'pointer') continue;
    if (el.closest(focusable) || el.querySelector(focusable)) continue;
    const text = (el.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 40);
    out.push(`${el.tagName.toLowerCase()}${typeof el.className === 'string' && el.className ? '.' + el.className.trim().split(/\s+/)[0] : ''} "${text}"`);
  }
  return out;
}

export interface FocusState {
  /** Id from `controlsInPage`, or a fresh one for a focusable the list did not know; 0 when focus is on the page itself. */
  id: number;
  label: string;
  /** How the focus shows (outline or shadow), or null when nothing does. */
  indicator: string | null;
  /** Has a size and is not hidden. */
  seen: boolean;
}

/** Where the focus is now and whether the person can see it. */
export function focusInPage(): FocusState {
  const w = window as unknown as { __a11y?: { ids: WeakMap<Element, number>; next: number } };
  w.__a11y ??= { ids: new WeakMap(), next: 1 };
  const el = document.activeElement as HTMLElement | null;
  if (!el || el === document.body || el === document.documentElement) return { id: 0, label: 'the page', indicator: null, seen: true };
  let id = w.__a11y.ids.get(el);
  if (!id) {
    id = w.__a11y.next++;
    w.__a11y.ids.set(el, id);
  }
  const cls = typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\s+/).slice(0, 2).join('.') : '';
  const text = (el.getAttribute('aria-label') ?? el.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 40);
  const label = `${el.tagName.toLowerCase()}${cls}${text ? ` "${text}"` : ''}`;

  const look = (target: Element): string | null => {
    const cs = getComputedStyle(target);
    if (cs.outlineStyle !== 'none' && parseFloat(cs.outlineWidth) >= 1 && !/, 0\)$/.test(cs.outlineColor)) return `outline ${cs.outlineWidth} ${cs.outlineColor}`;
    if (cs.boxShadow !== 'none') return `shadow ${cs.boxShadow}`;
    return null;
  };
  let indicator = look(el);
  // A control that is itself invisible (a checkbox under its switch) shows the focus on its picture.
  const r = el.getBoundingClientRect();
  const hiddenInput = el instanceof HTMLInputElement && (r.width <= 2 || r.height <= 2 || getComputedStyle(el).opacity === '0');
  if (!indicator && hiddenInput) {
    for (const near of [el.nextElementSibling, el.previousElementSibling, el.parentElement]) {
      if (near && (indicator = look(near))) break;
    }
  }
  const seen = el.checkVisibility({ checkVisibilityCSS: true }) && (r.width > 0 || hiddenInput) && (r.height > 0 || hiddenInput);
  return { id, label, indicator, seen };
}

export interface KeyboardReport {
  /** Tab stops the page offers. */
  stops: number;
  problems: string[];
}

/**
 * Tabs through the page from the top. Every control the browser puts in the Tab order must get the
 * focus (a radio group or a composite widget counts once), the focus must not stick (a trap), every
 * focused element must show a focus indicator, and no control may be left out of the Tab order or
 * work only with the mouse.
 */
export async function tabThrough(page: Page): Promise<KeyboardReport> {
  await page.evaluate(() => {
    (document.activeElement as HTMLElement | null)?.blur();
    window.scrollTo(0, 0);
  });
  const controls = await page.evaluate(controlsInPage);
  const problems: string[] = [];
  for (const c of controls) if (c.problem) problems.push(`${c.label}: ${c.problem}`);
  for (const m of await page.evaluate(mouseOnlyInPage)) problems.push(`${m}: reacts to the mouse (pointer cursor) but cannot be focused`);

  const tabbable = controls.filter((c) => c.tabbable);
  // Browsers tab to one radio of a group and to the active member of a composite widget: expect one stop per group.
  const groups = new Set<string>();
  let stops = 0;
  for (const c of tabbable) {
    if (c.group === null) stops += 1;
    else if (!groups.has(c.group)) {
      groups.add(c.group);
      stops += 1;
    }
  }
  const presses = Math.min(stops + 4, 400);
  const reached = new Set<number>();
  const noIndicator = new Set<string>();
  const notSeen = new Set<string>();
  let previous = -1;
  let stuck = 0;
  for (let i = 0; i < presses; i++) {
    await page.keyboard.press('Tab');
    const f = await page.evaluate(focusInPage);
    if (f.id === 0) {
      previous = 0;
      continue;
    }
    if (f.id === previous) stuck += 1;
    else stuck = 0;
    previous = f.id;
    if (stuck >= 1 && stops > 1) {
      problems.push(`focus trap: Tab does not leave ${f.label}`);
      break;
    }
    reached.add(f.id);
    if (f.indicator === null) noIndicator.add(f.label);
    if (!f.seen) notSeen.add(f.label);
  }
  const reachedGroups = new Set<string>();
  for (const c of tabbable) if (c.group !== null && reached.has(c.id)) reachedGroups.add(c.group);
  // Radios and widget members: the stop is any one of the group (a roving tabindex moves it).
  for (const c of tabbable) {
    if (reached.has(c.id)) continue;
    if (c.group !== null && reachedGroups.has(c.group)) continue;
    // Another member of the group may hold the stop: look at the other members before blaming this one.
    if (c.group !== null) {
      const stopHolder = tabbable.filter((o) => o.group === c.group);
      if (stopHolder.some((o) => reached.has(o.id))) continue;
    }
    problems.push(`${c.label}: never got the focus with Tab (unreachable by keyboard, or the focus was trapped before it)`);
  }
  for (const label of noIndicator) problems.push(`${label}: no visible focus indicator`);
  for (const label of notSeen) problems.push(`${label}: focus lands on something that is not visible`);
  return { stops, problems };
}

// ---------------------------------------------------------------- charts

export interface ChartFinding {
  chart: string;
  problem: string;
}

/**
 * Every chart (an SVG or canvas the person can see that is bigger than an icon, or anything with
 * `role="img"` that holds one) needs a name for assistive technology, and a summary: either the
 * name itself is a sentence that says what the chart shows, or a text next to it (`.chart-summary`
 * or `.sr-only`, in the same card) or `aria-describedby` does.
 */
export function chartsInPage(): ChartFinding[] {
  const out: ChartFinding[] = [];
  const visible = (el: Element) => (el as HTMLElement | SVGElement).checkVisibility({ checkVisibilityCSS: true });
  const nameOf = (el: Element): string => {
    const labelled = el.getAttribute('aria-labelledby');
    if (labelled) {
      return labelled
        .split(/\s+/)
        .map((id) => document.getElementById(id)?.textContent ?? '')
        .join(' ')
        .trim();
    }
    return (el.getAttribute('aria-label') ?? el.querySelector(':scope > title')?.textContent ?? '').trim();
  };
  const words = (s: string) => s.split(/\s+/).filter(Boolean).length;
  const describe = (el: Element) => {
    const cls = typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\s+/).slice(0, 2).join('.') : '';
    return `${el.tagName.toLowerCase()}${cls}`;
  };

  const seen = new Set<Element>();
  const candidates: Element[] = [];
  for (const el of document.querySelectorAll('svg, canvas, [role="img"]')) {
    if (!visible(el)) continue;
    // Hidden from assistive technology on purpose: the named parent stands for it.
    if (el.closest('[aria-hidden="true"]')) continue;
    const r = el.getBoundingClientRect();
    const isImg = el.getAttribute('role') === 'img';
    // An icon is not a chart.
    if (!isImg && (r.width < 40 || r.height < 24)) continue;
    if (isImg && r.width < 40 && r.height < 40 && !el.querySelector('svg, canvas')) continue;
    // The innermost named img (or labelled group, for the map that holds its own buttons) stands for everything inside it.
    const holder = el.closest('[role="img"], [role="group"][aria-label]');
    const target = holder ?? el;
    if (seen.has(target)) continue;
    seen.add(target);
    candidates.push(target);
  }

  for (const el of candidates) {
    const name = nameOf(el);
    const label = `${describe(el)}${name ? ` "${name.slice(0, 50)}"` : ''}`;
    if (!name) {
      out.push({ chart: label, problem: 'no accessible name (aria-label)' });
      continue;
    }
    const describedBy = el.getAttribute('aria-describedby');
    const scope = el.closest('section, article, .card, [role="group"], [role="region"], li') ?? el.parentElement ?? document.body;
    const text = scope.querySelector('.chart-summary, .sr-only')?.textContent?.trim() ?? '';
    // "Energy level unavailable" is the whole truth about a ring that has no value.
    const summarised = words(name) >= 4 || /unavailable|no data/i.test(name) || describedBy !== null || words(text) >= 4;
    // A plain label of one to three words ("Heart rate") names the chart but says nothing about its values.
    if (!summarised) out.push({ chart: label, problem: 'has a name but no summary of what it shows' });
  }
  return out;
}

// ---------------------------------------------------------------- axe

export interface AxeReport {
  rule: string;
  impact: string | null | undefined;
  targets: string[];
}

export async function runAxe(page: Page): Promise<AxeReport[]> {
  const results = await new AxeBuilder({ page }).withTags(AXE_TAGS).analyze();
  return results.violations.map((v) => ({ rule: v.id, impact: v.impact, targets: v.nodes.map((n) => n.target.join(' ')).slice(0, 5) }));
}
