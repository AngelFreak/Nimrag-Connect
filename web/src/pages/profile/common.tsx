import { Fragment, type ReactNode } from 'react';
import type { TokenInfo } from '../../api/generated/TokenInfo';
import { TopBar } from '../../components';
import { usePageTitle } from '../../lib/hooks';

/**
 * A page one level below the profile (or the watch): back button, title, then a narrow column. On a
 * phone it is a page of its own; from 1024 px it sits in the right-hand pane next to the section list.
 */
export function SubPage({ title, backTo, subtitle, aside, children }: { title: string; backTo: string; subtitle?: string; aside?: ReactNode; children: ReactNode }) {
  usePageTitle(title);
  return (
    <>
      <TopBar variant="back" backTo={backTo} title={title} {...(subtitle ? { subtitle } : {})} {...(aside ? { aside } : {})} />
      <div className="page">
        <div className="subpage">{children}</div>
      </div>
    </>
  );
}

/** The phones that are still signed in, the one this page is open on first, then the most recently used. */
export function activePhones(tokens: readonly TokenInfo[] | undefined): TokenInfo[] {
  return (tokens ?? [])
    .filter((t) => t.active)
    .sort((a, b) => Number(b.current) - Number(a.current) || (b.last_used_at ?? '').localeCompare(a.last_used_at ?? '') || b.id - a.id);
}

/** "Pixel 9a (this phone) and 1 more", "Pixel 9a", or "None"; one line for the profile and watch lists. */
export function phonesSummary(tokens: readonly TokenInfo[] | undefined): string {
  const phones = activePhones(tokens);
  const [first] = phones;
  if (!first) return 'None signed in';
  const name = first.current ? `${first.name} (this phone)` : first.name;
  const more = phones.length - 1;
  return more > 0 ? `${name} and ${more} more` : name;
}

/** A zone name that may break after each slash, so "America/Argentina/Buenos_Aires" wraps at a slash and not inside a word. */
export function breakableZone(zone: string): ReactNode {
  const parts = zone.split('/');
  return parts.map((part, i) => (
    <Fragment key={i}>
      {i > 0 ? '/' : ''}
      {i > 0 ? <wbr /> : null}
      {part}
    </Fragment>
  ));
}
