import { useTranslation } from 'react-i18next';

import type { StatusPageSubscriberImportReport } from '@/api/statusPages';

const KNOWN_REASONS = new Set(['missing_email', 'invalid_email', 'duplicate_email', 'unknown_component']);

/** What an import did, or would do when it was only checked, and the rows it left out. */
export function ImportReport({ report }: { report: StatusPageSubscriberImportReport }) {
  const { t } = useTranslation('status-pages');
  const facts = [
    { key: 'rows', value: report.rows },
    { key: report.dry_run ? 'would_import' : 'imported', value: report.imported },
    { key: 'already_subscribed', value: report.already_subscribed },
    { key: 'skipped_unsubscribed', value: report.skipped_unsubscribed },
    ...(report.dry_run ? [] : [{ key: 'confirmations_queued', value: report.confirmations_queued }]),
    { key: 'problems', value: report.problem_count },
  ];

  return (
    <div className="space-y-4" role="status">
      <dl className="grid grid-cols-2 gap-3 rounded-lg bg-bg-2 p-4 sm:grid-cols-3">
        {facts.map((fact) => (
          <div key={fact.key}>
            <dt className="text-xs text-tx-3">{t(`subscribers.import.report.${fact.key}`)}</dt>
            <dd className="mt-1 text-sm font-strong tabular-nums text-tx-0">{fact.value}</dd>
          </div>
        ))}
      </dl>
      {report.problems.length > 0 && (
        <div>
          <p className="text-xs text-tx-2">
            {report.problem_count > report.problems.length
              ? t('subscribers.import.problems_first', {
                  shown: report.problems.length,
                  count: report.problem_count,
                })
              : t('subscribers.import.problems_all', { count: report.problem_count })}
          </p>
          <ul className="mt-2 max-h-56 divide-y divide-bd-0 overflow-y-auto rounded-md bg-bg-1">
            {report.problems.map((problem) => (
              <li key={`${problem.line}-${problem.value}`} className="flex gap-3 px-3 py-2 text-xs">
                <span className="w-16 shrink-0 tabular-nums text-tx-3">
                  {t('subscribers.import.line', { line: problem.line })}
                </span>
                <span className="min-w-0 flex-1 truncate text-tx-1">{problem.value || t('values.none')}</span>
                <span className="shrink-0 text-tx-2">
                  {KNOWN_REASONS.has(problem.reason)
                    ? t(`subscribers.import.reasons.${problem.reason}`)
                    : problem.reason}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
