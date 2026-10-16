import type { TFunction } from 'i18next';

import type { PublicStatusPagePostmortem, StatusPageLanguage } from '@/api/statusPages';
import { MarkdownMessage } from '@/routes/agent/markdown';

import { formatMicros } from '../model';
import { microsDateTime } from './format';

/** What the publisher says about an incident once it is over: why it happened and what changes. */
export function IncidentPostmortem({
  postmortem,
  timezone,
  language,
  copy,
}: {
  postmortem: PublicStatusPagePostmortem;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  const revised = postmortem.revised_at !== null && postmortem.revised_at !== postmortem.published_at;

  return (
    <section className="border-t border-bd-0 px-5 py-7 sm:px-8 sm:py-9" aria-labelledby="incident-postmortem-heading">
      <h3 id="incident-postmortem-heading" className="text-lg font-display-strong tracking-[-0.01em] text-tx-0 sm:text-xl">
        {copy('public.incident_detail.postmortem')}
      </h3>
      {postmortem.published_at !== null && (
        <p className="mt-1.5 text-sm text-tx-2">
          <time dateTime={microsDateTime(postmortem.published_at)} className="tabular-nums">
            {copy('public.incident_detail.postmortem_published', {
              time: formatMicros(postmortem.published_at, timezone, language),
            })}
          </time>
          {revised && postmortem.revised_at !== null && (
            <>
              {' · '}
              <time dateTime={microsDateTime(postmortem.revised_at)} className="tabular-nums">
                {copy('public.incident_detail.postmortem_revised', {
                  time: formatMicros(postmortem.revised_at, timezone, language),
                })}
              </time>
            </>
          )}
        </p>
      )}
      <MarkdownMessage content={postmortem.body} className="mt-5 max-w-3xl gap-3 text-base leading-7" />
    </section>
  );
}
