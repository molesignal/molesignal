import { AlertTriangle, CheckCircle2, Route } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { NotifyPolicyPreview } from '@/api/notify';
import { Card, CardBody, CardHeader, Pill } from '@/shell/chrome';

export function PolicyPreview({
  preview,
  error,
  loading,
}: {
  preview: NotifyPolicyPreview | null;
  error: string | null;
  loading: boolean;
}) {
  const { t } = useTranslation('notify');
  return (
    <Card className="sticky top-4">
      <CardHeader
        title={t('policies.drawer.preview')}
        actions={loading ? <Pill tone="blue">{t('common.loading')}</Pill> : undefined}
      />
      <CardBody className="space-y-3">
        {error && (
          <div className="flex gap-2 rounded-md bg-red-dim p-3 text-xs leading-relaxed text-red-soft">
            <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            {error}
          </div>
        )}
        {!error && !preview && (
          <p className="text-xs leading-relaxed text-tx-3">
            {t('policies.drawer.preview_waiting')}
          </p>
        )}
        {preview && (
          <>
            <div className="flex items-center gap-2">
              <CheckCircle2
                className={preview.matched ? 'h-4 w-4 text-green' : 'h-4 w-4 text-tx-3'}
              />
              <span className="text-sm font-semibold text-tx-0">
                {t(
                  preview.matched
                    ? 'policies.drawer.matched'
                    : 'policies.drawer.not_matched',
                )}
              </span>
            </div>
            {preview.recipients.length === 0 ? (
              <p className="text-xs text-tx-3">{t('policies.drawer.no_recipients')}</p>
            ) : (
              <div className="space-y-3">
                {preview.recipients.map((recipient) => (
                  <div
                    key={`${recipient.user_id}:${recipient.team_id ?? ''}`}
                    className="rounded-md border border-bd-0 bg-bg-2 p-3"
                  >
                    <div className="truncate font-mono text-xs font-semibold text-tx-1">
                      {recipient.user_id}
                    </div>
                    <div className="mt-1 text-xs text-tx-3">
                      {t('policies.drawer.resolved_by', {
                        resolver: t(
                          `resolver_types.${recipient.resolved_by}`,
                          { defaultValue: recipient.resolved_by },
                        ),
                      })}
                    </div>
                    <div className="mt-2 space-y-2">
                      {recipient.delivery_plan.map((step, index) => (
                        <div
                          key={`${step.stage}:${step.connector_id}:${step.endpoint_id ?? index}`}
                          className="flex min-w-0 items-start gap-2"
                        >
                          <Route className="mt-0.5 h-3.5 w-3.5 shrink-0 text-indigo-soft" />
                          <div className="min-w-0 text-xs">
                            <div className="truncate font-semibold text-tx-1">
                              {step.connector_name}
                            </div>
                            <div className="truncate text-tx-3">
                              {t(`stages.${step.stage}`)} · {step.target_value_masked}
                            </div>
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </CardBody>
    </Card>
  );
}
