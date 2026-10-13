import { useTranslation } from 'react-i18next';

import { formatMicrosActive } from '@/lib/time';
import { Pill } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import type { Incident } from '@/types/alerting';

function labelsToString(labels: Record<string, string>): string {
  return (
    Object.entries(labels)
      .map(([key, value]) => `${key}="${value}"`)
      .join(' · ') || '—'
  );
}

/** The query whose result opened the incident, and the samples that crossed its threshold. */
export function TriggeringQueryBlock({ query }: { query: NonNullable<Incident['triggering_query']> }) {
  const { t } = useTranslation('alerts');

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-1.5">
        <Pill tone="indigo">{query.language.toUpperCase()}</Pill>
        <span className="font-sans text-xs text-tx-2">
          {t('drawer.triggering_query.sample_count', { count: query.sample_values.length })}
        </span>
      </div>
      <pre className="overflow-auto rounded-md bg-bg-2 px-3 py-2.5 font-mono text-xs leading-relaxed text-tx-0">
        <code>{query.statement}</code>
      </pre>
      {query.sample_values.length > 0 && (
        <details>
          <summary
            className={cn(
              'w-fit cursor-pointer list-none font-sans text-xs font-strong text-tx-1 transition-colors',
              'hover:text-indigo-soft focus-visible:text-indigo-soft',
              '[&::-webkit-details-marker]:hidden',
            )}
          >
            {t('drawer.triggering_query.show_samples')}
          </summary>
          <ul className="mt-2 rounded-md bg-bg-2 px-3 py-1.5 font-sans text-xs tabular-nums">
            {query.sample_values.map((sample, index) => (
              <li key={index} className="flex justify-between gap-3 border-b border-bd-0 py-1.5 last:border-b-0">
                <span className="shrink-0 text-tx-2">{formatMicrosActive(sample.ts)}</span>
                <span className="min-w-0 truncate text-tx-2">{labelsToString(sample.labels)}</span>
                <span className="shrink-0 font-strong text-tx-0">{sample.value.toFixed(3)}</span>
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
