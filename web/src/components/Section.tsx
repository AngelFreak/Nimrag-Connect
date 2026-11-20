import { useId, type ReactNode } from 'react';
import { AlertIcon } from './icons';

interface SectionProps {
  title: ReactNode;
  /** One plain sentence under the title: what belongs in this group. */
  hint?: ReactNode;
  children: ReactNode;
}

/**
 * A titled group of cards (Settings: Account, Goals, Phones, Garmin). On a phone the title sits
 * above the cards; from 1024 px it moves into a left column that stays in view while the cards
 * scroll, so a long page keeps its map. The title is an h2: the cards inside use h3.
 */
export function Section({ title, hint, children }: SectionProps) {
  const id = useId();
  return (
    <section className="section" aria-labelledby={id}>
      <div className="section__head">
        <h2 className="section__title" id={id}>
          {title}
        </h2>
        {hint ? <p className="section__hint">{hint}</p> : null}
      </div>
      <div className="section__cards">{children}</div>
    </section>
  );
}

interface NoticeProps {
  tone?: 'info' | 'warn';
  children: ReactNode;
}

/**
 * A short boxed note inside a card, for what a person should know before acting (the unofficial
 * Garmin login). Not an alert: nothing went wrong, so it is not announced.
 */
export function Notice({ tone = 'info', children }: NoticeProps) {
  return (
    <div className={tone === 'warn' ? 'notice notice--warn' : 'notice'}>
      {tone === 'warn' ? <AlertIcon size={18} /> : null}
      <p>{children}</p>
    </div>
  );
}
