import type * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { useReportRenderState } from './readiness';

/** Something the report says about itself under its title: the range it covers, when it was made. */
export interface ReportFact {
  label: string;
  value: React.ReactNode;
}

/**
 * A report as it is printed: what it is, what it covers, and then the thing
 * itself.
 *
 * Nothing of the application is on the page: no navigation, no toolbar, no
 * control that would do nothing on paper. The page is as long as its content,
 * which is what the renderer measures to size the PDF or the image.
 */
export function ReportDocument({
  title,
  description,
  facts = [],
  children,
  className,
}: {
  title: string;
  description?: string | undefined;
  facts?: readonly ReportFact[];
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <article
      data-report-document-body
      className={cn('bg-[var(--functional-surface)] px-[24px] pb-[24px] pt-[22px] text-tx-0', className)}
    >
      <header className="mb-[16px] border-b border-bd-1 pb-[16px]">
        <h1 className="text-pretty font-sans text-2xl font-display tracking-[-0.02em] text-tx-0">
          {title}
        </h1>
        {description && (
          <p className="mt-[6px] max-w-[96ch] text-pretty font-sans text-sm text-tx-2">{description}</p>
        )}
        {facts.length > 0 && (
          <dl className="mt-[14px] flex flex-wrap gap-x-[36px] gap-y-[10px]">
            {facts.map((fact, index) => (
              // Labels come from the report's subject and can repeat: two variables may share one.
              <div key={`${index}:${fact.label}`} className="min-w-0">
                <dt className="type-label font-medium text-tx-3">{fact.label}</dt>
                <dd className="mt-[3px] font-sans text-sm font-strong tabular-nums text-tx-0">
                  {fact.value}
                </dd>
              </div>
            ))}
          </dl>
        )}
      </header>
      {children}
    </article>
  );
}

/**
 * A report that cannot be rendered. The renderer is told why and fails the
 * report with that reason; a person who opens the page reads the same thing.
 */
export function ReportUnavailable({ reason, error }: { reason: string; error?: unknown }) {
  useReportRenderState('error', error ?? reason);

  return (
    <p role="alert" className="bg-[var(--functional-surface)] px-[24px] py-[22px] font-sans text-sm text-tx-1">
      {reason}
    </p>
  );
}
