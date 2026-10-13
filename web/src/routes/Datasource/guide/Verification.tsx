import { CircleAlert, CircleCheck, LoaderCircle, RefreshCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';

import type { SourceSignalSummary } from '../datasourceModel';
import type { IntakeContext } from '../intakeContext';
import type { Source } from '../sources';
import {
  formatBytes,
  formatCount,
  formatEventRate,
  formatRelativeMicros,
  RECEIPT_WINDOW_SECS,
  type ValidationState,
} from './receipt';
import { TestEventButton } from './TestEvent';

type Outcome = 'idle' | 'success' | 'warning' | 'error';

const OUTCOME_FILL: Record<Outcome, string> = {
  idle: 'bg-[var(--control-surface)]',
  success: 'bg-[var(--green-tint)]',
  warning: 'bg-[var(--yellow-tint)]',
  error: 'bg-red-dim',
};

const OUTCOME_TEXT: Record<Outcome, string> = {
  idle: 'text-tx-0',
  success: 'text-[var(--green-ink)]',
  warning: 'text-[var(--yellow-ink)]',
  error: 'text-red-soft',
};

/**
 * Whether the source's data is arriving: the one check of the guide, what it
 * found, and where to go from there. `passiveSummary` is what the workspace
 * has received without anyone asking, so data that already flows is shown
 * before the check is run.
 */
export function Verification({
  source,
  validation,
  passiveSummary,
  context,
  viewPath,
  onValidate,
  children,
}: {
  source: Source;
  validation: ValidationState;
  passiveSummary: SourceSignalSummary;
  context: IntakeContext;
  /** Where the received data can be looked at. */
  viewPath: string;
  onValidate: () => void;
  /** The checks the reader runs where the collector is deployed. */
  children?: React.ReactNode;
}) {
  const { t, i18n } = useTranslation('onboarding');
  const navigate = useNavigate();
  const pipelineCreateAccess = useActionAccess({ permission: 'pipelines.create' });

  const checked = validation.kind === 'success' || validation.kind === 'warning';
  const summary = checked ? validation.summary : passiveSummary;
  const health = checked ? validation.health : null;
  const outcome: Outcome =
    validation.kind === 'pending' || validation.kind === 'idle' ? 'idle' : validation.kind;
  const pending = validation.kind === 'pending';
  const blocked = context.isRum && (!context.applicationValid || !context.token);

  const facts: Array<{ label: string; value: string; healthy?: boolean }> = [];

  if (health?.ok) {
    facts.push({
      label: t('datasource_page.validation_metrics.receiver'),
      value: t('datasource_page.receiver_healthy', { ms: health.latencyMs }),
      healthy: true,
    });
  }
  if (summary.rows > 0) {
    facts.push(
      {
        label: t('datasource_page.validation_metrics.last_received'),
        value: formatRelativeMicros(summary.lastReceivedAtMicros, i18n.language),
      },
      {
        label: t('datasource_page.validation_metrics.streams'),
        value: summary.streamNames.length > 0 ? summary.streamNames.slice(0, 3).join(', ') : '—',
      },
      {
        label: t('datasource_page.validation_metrics.volume'),
        value: t('datasource_page.validation_volume', {
          rate: formatEventRate(summary.rows, RECEIPT_WINDOW_SECS),
          rows: formatCount(summary.rows),
          bytes: formatBytes(summary.storedBytes),
        }),
      },
    );
  }

  const OutcomeIcon = outcome === 'success' ? CircleCheck : CircleAlert;

  return (
    <div className="min-w-0 space-y-[20px]">
      <div
        data-verification={pending ? 'pending' : outcome}
        className={cn('rounded-md px-[16px] py-[14px]', OUTCOME_FILL[outcome])}
      >
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
          <div className="flex min-w-0 flex-1 items-start gap-[10px]">
            {outcome !== 'idle' && (
              <OutcomeIcon
                aria-hidden
                className={cn('mt-[2px] h-4 w-4 shrink-0', OUTCOME_TEXT[outcome])}
              />
            )}
            <div className="min-w-0" role="status">
              <div className={cn('font-sans text-md font-strong', OUTCOME_TEXT[outcome])}>
                {t(`datasource_page.validation_state.${outcome}.title`)}
              </div>
              <div className="mt-0.5 font-sans text-sm leading-relaxed text-tx-2">
                {validation.kind === 'error'
                  ? validation.message
                  : t(`datasource_page.validation_state.${outcome}.description`, {
                      active: summary.activeSignals,
                      total: summary.expectedSignals,
                    })}
              </div>
            </div>
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-2">
            <ChromeButton
              variant={outcome === 'success' ? 'default' : 'primary'}
              disabled={pending || blocked}
              onClick={onValidate}
            >
              {pending ? (
                <LoaderCircle aria-hidden className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <RefreshCw aria-hidden className="h-3.5 w-3.5" />
              )}
              {pending
                ? t('datasource_page.validating')
                : outcome === 'idle'
                  ? t('datasource_page.verify_connection')
                  : t('datasource_page.recheck')}
            </ChromeButton>
            {outcome === 'success' && (
              <>
                <ChromeButton
                  disabled={pipelineCreateAccess.disabled}
                  disabledReason={pipelineCreateAccess.reason}
                  onClick={() => navigate('/pipelines/new')}
                >
                  {t('datasource_page.create_pipeline')}
                </ChromeButton>
                <ChromeButton variant="primary" onClick={() => navigate(viewPath)}>
                  {t('datasource_page.view_received_data')}
                </ChromeButton>
              </>
            )}
          </div>
        </div>

        {facts.length > 0 && (
          <dl
            data-verification-facts
            className="m-0 mt-[14px] flex flex-wrap gap-x-[32px] gap-y-[10px]"
          >
            {facts.map((fact) => (
              <div key={fact.label} className="min-w-0">
                <dt className="font-sans text-xs text-tx-3">{fact.label}</dt>
                <dd
                  title={fact.value}
                  className={cn(
                    'm-0 mt-0.5 max-w-[320px] truncate font-sans text-sm font-strong text-tx-1',
                    fact.healthy && 'text-[var(--green-ink)]',
                  )}
                >
                  {fact.value}
                </dd>
              </div>
            ))}
          </dl>
        )}
      </div>

      {(outcome === 'warning' || outcome === 'error') && (
        <div data-verification-causes>
          <div className={uiLabelClass}>{t('datasource_page.possible_causes')}</div>
          <ul className="m-0 mt-2 list-disc space-y-1.5 pl-[18px] font-sans text-sm leading-relaxed text-tx-2">
            <li>{t('datasource_page.cause_collector')}</li>
            <li>{t('datasource_page.cause_token')}</li>
            <li>{t('datasource_page.cause_network')}</li>
          </ul>
          <div className="mt-3 flex flex-wrap items-center gap-2 empty:hidden">
            <TestEventButton source={source} context={context} />
          </div>
        </div>
      )}

      {children}
    </div>
  );
}
