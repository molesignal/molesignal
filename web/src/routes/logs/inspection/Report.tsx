import { CircleAlert } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { evidenceHref, parseStructuredAnswer, type StructuredAnswer } from '@/routes/agent/answer';
import { MarkdownMessage } from '@/routes/agent/markdown';

interface InspectionReportProps {
  /** What the model wrote. */
  content: string;
}

const HEADING = 'font-sans text-xs font-strong text-tx-2';
const ACTION =
  'inline-flex min-h-8 items-center rounded-md bg-[var(--control-surface)] px-2.5 py-1 font-sans text-xs font-strong text-[var(--blue-ink)] hover:bg-bg-3 focus-visible:bg-bg-3';

function Part({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-1.5">
      <h4 className={HEADING}>{title}</h4>
      {children}
    </section>
  );
}

function confidenceLevel(confidence: NonNullable<StructuredAnswer['confidence']>): 'high' | 'medium' | 'low' {
  if (typeof confidence === 'string') return confidence;

  return confidence >= 0.75 ? 'high' : confidence >= 0.45 ? 'medium' : 'low';
}

/**
 * The report of an inspection. Mole Agent answers in a fixed shape (a
 * conclusion, the facts it rests on, likely causes, what could not be
 * confirmed, what to do next), and the report is laid out part by part in
 * that order, named as the same parts are in a Mole Agent chat. An answer that
 * is not in that shape is shown as it was written.
 */
export function InspectionReport({ content }: InspectionReportProps) {
  const { t } = useTranslation('agent');
  const answer = React.useMemo(() => parseStructuredAnswer(content), [content]);

  if (!answer) return <MarkdownMessage content={content} className="font-sans text-sm text-tx-1" />;

  const facts = answer.evidence ?? [];
  const points = answer.anomaly_points ?? [];
  const links = answer.related_links ?? [];

  return (
    <div data-log-inspection-report className="flex flex-col gap-4 font-sans text-sm leading-relaxed text-tx-1">
      {answer.summary && (
        <Part title={t('answer.conclusion')}>
          <p className="font-strong text-tx-0">{answer.summary}</p>
        </Part>
      )}
      {points.length > 0 && (
        <Part title={t('answer.anomaly_points')}>
          <ul className="flex flex-col gap-1">
            {points.map((point, index) => (
              <li key={index} className="rounded-md bg-[var(--control-surface)] px-3 py-2">
                <span className="font-strong text-tx-0">{point.metric ?? point.stream ?? '—'}</span>
                {point.observed && (
                  <span className="text-tx-2">
                    {' · '}
                    {point.observed}
                    {point.expected ? ` (${point.expected})` : ''}
                  </span>
                )}
                {point.description && <div className="text-tx-2">{point.description}</div>}
              </li>
            ))}
          </ul>
        </Part>
      )}
      {facts.length > 0 && (
        <Part title={t('answer.evidence')}>
          <ul className="flex flex-col gap-1">
            {facts.map((fact, index) => {
              const href = evidenceHref(fact);
              const label = fact.label ?? fact.query ?? fact.stream ?? '—';

              return (
                <li key={index} className="flex items-baseline gap-2 rounded-md bg-[var(--control-surface)] px-3 py-2">
                  {fact.kind && <span className="type-micro shrink-0 font-strong uppercase text-tx-3">{fact.kind}</span>}
                  {href ? (
                    <Link to={href} className="min-w-0 text-[var(--blue-ink)] hover:underline focus-visible:underline">
                      {label}
                    </Link>
                  ) : (
                    <span className="min-w-0">{label}</span>
                  )}
                </li>
              );
            })}
          </ul>
        </Part>
      )}
      {answer.likely_causes && answer.likely_causes.length > 0 && (
        <Part title={t('answer.likely_causes')}>
          <ol className="ml-4 list-decimal space-y-0.5">
            {answer.likely_causes.map((cause, index) => (
              <li key={index}>{cause}</li>
            ))}
          </ol>
        </Part>
      )}
      {answer.limitations && answer.limitations.length > 0 && (
        <Part title={t('answer.limitations')}>
          <ul className="space-y-1">
            {answer.limitations.map((limitation, index) => (
              <li key={index} className="flex items-start gap-2 text-tx-2">
                <CircleAlert className="mt-[3px] h-3.5 w-3.5 shrink-0 text-[var(--yellow-ink)]" aria-hidden="true" />
                <span>{limitation}</span>
              </li>
            ))}
          </ul>
        </Part>
      )}
      {answer.suggested_next_steps && answer.suggested_next_steps.length > 0 && (
        <Part title={t('answer.next_steps')}>
          <ol className="ml-4 list-decimal space-y-0.5">
            {answer.suggested_next_steps.map((step, index) => (
              <li key={index}>{step}</li>
            ))}
          </ol>
        </Part>
      )}
      {links.length > 0 && (
        <Part title={t('answer.related_links')}>
          <ul className="flex flex-wrap gap-2">
            {links.map((link, index) => {
              const href = link.href ?? link.route;

              return (
                <li key={index}>
                  {href ? (
                    <Link to={href} className={ACTION}>
                      {link.label}
                    </Link>
                  ) : (
                    <span className="inline-flex min-h-8 items-center px-1 text-xs text-tx-2">{link.label}</span>
                  )}
                </li>
              );
            })}
          </ul>
        </Part>
      )}
      {answer.confidence !== undefined && (
        <p className="font-sans text-xs text-tx-2">
          {t('answer.confidence')}{' '}
          <span className="font-strong text-tx-0">
            {t(`answer.confidence_levels.${confidenceLevel(answer.confidence)}`)}
          </span>
        </p>
      )}
    </div>
  );
}

/**
 * Whether what has arrived so far is the fixed shape still being written. Half
 * of a JSON object is not shown: the reader waits for the report it becomes.
 */
export function isReportInTheMaking(content: string): boolean {
  const text = content.trimStart();

  return text.startsWith('{') || text.startsWith('```');
}
