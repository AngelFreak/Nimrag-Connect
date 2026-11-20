// The layout audit's measuring code (used by real.layout.spec.ts). `auditLayout` runs inside the
// page and returns everything that makes a page look broken on a phone: sideways overflow of the
// page, elements that stick out past the viewport, scrollbars nobody asked for, content clipped by
// `overflow: hidden`, and controls too small to hit.
//
// Scrolling that IS wanted is marked in the markup with `data-scroll="x"` (a chip row, a carousel,
// a wide table that scrolls inside a region) or `data-scroll="y"` (a dialog taller than the screen);
// the marker covers everything inside the element too.

export type FindingKind = 'page-overflow' | 'past-edge' | 'inner-scroll-x' | 'inner-scroll-y' | 'clipped' | 'small-target';

export interface Finding {
  kind: FindingKind;
  /** tag#id.class "text" of the element, short enough to find it. */
  element: string;
  detail: string;
}

/** Smallest touch target, in CSS px (the design's --touch). */
export const MIN_TARGET = 44;

/** Runs in the browser. Keep it self-contained: Playwright serialises the function. */
export function auditLayout(minTarget: number): Finding[] {
  const out: Finding[] = [];
  const vw = window.innerWidth;
  const root = document.documentElement;

  const describe = (el: Element): string => {
    const id = el.id ? `#${el.id}` : '';
    const cls = typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\s+/).slice(0, 2).join('.') : '';
    const text = (el.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 40);
    return `${el.tagName.toLowerCase()}${id}${cls}${text ? ` "${text}"` : ''}`;
  };
  const push = (kind: FindingKind, el: Element, detail: string) => out.push({ kind, element: describe(el), detail });

  if (root.scrollWidth > vw + 0.5) push('page-overflow', root, `scrollWidth ${root.scrollWidth} > viewport ${vw}`);
  if (document.body.scrollWidth > vw + 0.5) push('page-overflow', document.body, `body scrollWidth ${document.body.scrollWidth} > viewport ${vw}`);

  const wantedScroll = (el: Element, axis: 'x' | 'y') => {
    const mark = el.closest('[data-scroll]')?.getAttribute('data-scroll') ?? '';
    return mark.split(/\s+/).includes(axis);
  };
  // A 1 px box is a visually hidden label (clip-path), not something a person sees.
  const visible = (cs: CSSStyleDeclaration, r: DOMRect) => cs.display !== 'none' && cs.visibility !== 'hidden' && r.width > 1.5 && r.height > 1.5;
  const srOnly = (el: Element) => el.closest('.sr-only, .skip-link') !== null;

  /**
   * Is the element clipped horizontally by an ancestor (sticking out is then invisible, and that
   * ancestor is checked itself)? An absolutely placed element is only clipped by its containing
   * block and what is outside it: a `.sr-only` cell placed next to a scrolling table is not clipped
   * by the table unless the table is positioned, which is how a hidden label once widened a page.
   */
  const clippedByAncestor = (el: Element, cs: CSSStyleDeclaration) => {
    if (cs.position === 'fixed') return false;
    let p = el.parentElement;
    if (cs.position === 'absolute') {
      while (p && p !== document.body && p !== root) {
        const pcs = getComputedStyle(p);
        const containing = pcs.position !== 'static' || pcs.transform !== 'none' || /layout|paint|strict|content/.test(pcs.contain);
        if (containing) break;
        p = p.parentElement;
      }
    }
    for (; p && p !== document.body && p !== root; p = p.parentElement) {
      if (getComputedStyle(p).overflowX !== 'visible') return true;
    }
    return false;
  };

  for (const el of Array.from(document.body.querySelectorAll('*'))) {
    if (el.closest('svg') && el.tagName.toLowerCase() !== 'svg') continue;
    if (srOnly(el)) continue;
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    if (!visible(cs, r)) continue;

    if ((r.right > vw + 0.5 || r.left < -0.5) && !clippedByAncestor(el, cs) && !wantedScroll(el, 'x')) {
      push('past-edge', el, `x ${Math.round(r.left)}..${Math.round(r.right)} of ${vw}`);
    }

    const ox = cs.overflowX;
    const oy = cs.overflowY;
    if (el.scrollWidth > el.clientWidth + 1 && el.clientWidth > 0) {
      if ((ox === 'auto' || ox === 'scroll') && !wantedScroll(el, 'x')) push('inner-scroll-x', el, `content ${el.scrollWidth} in ${el.clientWidth}`);
      else if ((ox === 'hidden' || ox === 'clip') && cs.textOverflow !== 'ellipsis' && !wantedScroll(el, 'x')) push('clipped', el, `content ${el.scrollWidth} cut to ${el.clientWidth}`);
    }
    if ((oy === 'auto' || oy === 'scroll') && el.scrollHeight > el.clientHeight + 1 && !wantedScroll(el, 'y')) {
      push('inner-scroll-y', el, `content ${el.scrollHeight} in ${el.clientHeight}`);
    }

    // Touch targets. A link inside running text is exempt (WCAG 2.5.8); a checkbox is hit through its label.
    const tag = el.tagName.toLowerCase();
    const interactive =
      tag === 'button' ||
      tag === 'select' ||
      tag === 'textarea' ||
      tag === 'summary' ||
      (tag === 'a' && el.hasAttribute('href')) ||
      el.getAttribute('role') === 'button' ||
      (tag === 'input' && !['hidden', 'file'].includes((el as HTMLInputElement).type));
    if (interactive && !(tag === 'a' && cs.display === 'inline')) {
      const label = tag === 'input' ? el.closest('label') : null;
      const tr = (label ?? el).getBoundingClientRect();
      if (Math.min(tr.width, tr.height) < minTarget - 0.5) push('small-target', el, `${Math.round(tr.width)}x${Math.round(tr.height)}`);
    }
  }
  return out;
}
