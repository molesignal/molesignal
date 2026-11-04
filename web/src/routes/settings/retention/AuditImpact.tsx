import { useTranslation } from 'react-i18next';

import type { AuditRetentionPreview } from '@/api/instance';
import { useTimeFormatter } from '@/lib/time';

import { ImpactFailure, ImpactStatus } from './ImpactStatus';
import type { RetentionPreviewState } from './model';

/** The facts for the confirmation. Audit records are counted, so these are not estimates. */
export function useAuditImpactFacts(preview: AuditRetentionPreview | null) {
  const { t, i18n } = useTranslation('settings-admin');
  const time = useTimeFormatter();

  if (!preview) return undefined;

  return [
    { label: t('audit_retention.confirm.records'), value: preview.rows.toLocaleString(i18n.language) },
    { label: t('audit_retention.confirm.earliest'), value: time.micros(preview.earliestAt, false) },
  ];
}

/** Under the days to keep while a shorter retention is typed: how many records it would delete. */
export function AuditImpact({ impact }: { impact: RetentionPreviewState<AuditRetentionPreview> }) {
  const { t, i18n } = useTranslation('settings-admin');
  const time = useTimeFormatter();

  if (impact.status === 'idle') return null;

  return (
    <div data-retention-preview className="mt-4">
      {impact.status === 'pending' && <ImpactStatus>{t('audit_retention.preview.counting')}</ImpactStatus>}
      {impact.status === 'failed' && (
        <ImpactFailure onRetry={impact.retry}>{t('audit_retention.preview.failed')}</ImpactFailure>
      )}
      {impact.status === 'ready' && (
        <ImpactStatus>
          {impact.preview.rows === 0
            ? t('audit_retention.preview.nothing')
            : t('audit_retention.preview.found', {
                count: impact.preview.rows,
                rows: impact.preview.rows.toLocaleString(i18n.language),
                earliest: time.micros(impact.preview.earliestAt, false),
              })}
        </ImpactStatus>
      )}
    </div>
  );
}
