import * as React from 'react';
import { Trans, useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';

import { type KindTotal, type Statement } from './model';
import { formatCount, formatPercent } from '../overview/format';
import { KIND_TEXT } from '../overview/Frustration';
import { FIGURE } from '../performance/Reading';

/** A number of sessions with its noun: "1 session", "296 sessions", the figure set in the strong step. */
function Sessions({ value, language }: { value: number; language: string }) {
  return (
    <Trans
      i18nKey="frustration.statement.sessions"
      ns="rum"
      count={value}
      values={{ formatted: formatCount(value, language) }}
      components={{ n: FIGURE }}
    />
  );
}

/** One sign with how many actions showed it and in how many sessions, the name of the sign in its own colour. */
function Kind({ total, browser, language }: { total: KindTotal; browser: boolean; language: string }) {
  return (
    <Trans
      i18nKey={`frustration.statement.kinds.${browser ? total.kind : `${total.kind}_tap`}`}
      ns="rum"
      count={total.actions}
      values={{ formatted: formatCount(total.actions, language) }}
      components={{
        n: FIGURE,
        k: <span data-rum-kind={total.kind} className={cn('font-strong', KIND_TEXT[total.kind])} />,
        sessions: <Sessions value={total.sessions} language={language} />,
      }}
    />
  );
}

/**
 * The opening statement of the tab: what share of the sessions showed a sign of frustration, and for
 * each sign that occurred how many actions showed it and in how many sessions. A person who clicks ten
 * times is one session, so the sessions are the figure the table below is ordered by.
 */
export function FrustrationStatement({
  statement,
  share,
  browser,
  language,
}: {
  statement: Statement;
  /** Of all the sessions; none where they could not be counted. */
  share: number | null;
  browser: boolean;
  language: string;
}) {
  const { t } = useTranslation('rum');

  const kinds = statement.kinds.map((total) => <Kind key={total.kind} total={total} browser={browser} language={language} />);

  return (
    <section aria-label={t('frustration.statement.aria')} data-rum-frustration-statement="" className={cn(surfacePanelClass, 'px-5 py-4')}>
      <p data-rum-statement="" className="m-0 type-page-title font-sans font-strong text-tx-1">
        <Trans
          i18nKey={share === null ? 'frustration.statement.sentence_count' : 'frustration.statement.sentence'}
          ns="rum"
          {...(share === null && { count: statement.sessions })}
          values={{ share: share === null ? '' : formatPercent(share), sessions: formatCount(statement.sessions, language) }}
          components={{
            share: <span className="font-display text-[var(--orange-ink)]" />,
            n: FIGURE,
            kinds: (
              <>
                {kinds.map((kind, index) => (
                  <React.Fragment key={index}>
                    {index > 0 && t('frustration.statement.join')}
                    {kind}
                  </React.Fragment>
                ))}
              </>
            ),
          }}
        />
      </p>
    </section>
  );
}
