import { useTranslation } from 'react-i18next';

import type { Span } from '@/api/web';

/**
 * What the span says of how it was recorded rather than of what it did: the
 * library that recorded it, what was dropped, why it was kept. Only what it
 * holds is listed.
 */
export function SpanMetadata({ span }: { span: Span }) {
  const { t } = useTranslation('traces');

  const facts: Array<[string, string]> = [];
  const scope = [span.scope?.name, span.scope?.version].filter(Boolean).join(' ');

  if (scope) facts.push([t('view.metadata.scope'), scope]);
  if (span.traceState) facts.push([t('view.metadata.trace_state'), span.traceState]);
  facts.push([t('view.metadata.trace_flags'), String(span.traceFlags ?? 0)]);
  if (span.samplingReason) facts.push([t('view.metadata.sampling'), String(span.samplingReason)]);

  for (const [key, count] of [
    ['dropped_attributes', span.droppedAttributesCount],
    ['dropped_events', span.droppedEventsCount],
    ['dropped_links', span.droppedLinksCount],
  ] as const) {
    if (count > 0) facts.push([t(`view.metadata.${key}`), String(count)]);
  }

  const notes = (['partial', 'late', 'duplicate', 'conflict'] as const).filter((flag) => span[flag]);
  if (notes.length > 0) facts.push([t('view.metadata.notes'), notes.map((flag) => t(`view.metadata.flags.${flag}`)).join(' · ')]);

  return (
    <section aria-label={t('view.metadata.title')} className="mt-3">
      <h3 className="px-[6px] pb-[2px] font-sans text-xs font-strong text-tx-3">{t('view.metadata.title')}</h3>
      <dl className="min-w-0">
        {facts.map(([label, value]) => (
          <div key={label} className="grid min-w-0 grid-cols-[minmax(0,2fr)_minmax(0,3fr)] gap-[8px] rounded-sm px-[6px] py-[5px]">
            <dt className="truncate font-sans text-xs leading-[18px] text-tx-2">{label}</dt>
            <dd className="truncate font-sans text-xs leading-[18px] text-tx-0" title={value}>{value}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
