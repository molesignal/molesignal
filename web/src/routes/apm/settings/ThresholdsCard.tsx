import { Gauge } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ApmSettings } from '@/api/apmSettings';
import type { ActionAccess } from '@/product/actionAccess';

import { parseNumber, thresholdIssues } from './form';
import { CardSave, Issues, NumberField, useDraft } from './parts';
import { ApmCard } from '../frame/Page';

type Thresholds = Pick<ApmSettings, 'health' | 'apdexThreshold' | 'longTaskTime'>;

/** The thresholds as typed: rates in percent, durations in milliseconds. */
interface Typed {
  error_warning: string;
  error_critical: string;
  p95_warning: string;
  p95_critical: string;
  apdex: string;
  long_task: string;
}

const percent = (fraction: number) => String(Math.round(fraction * 10_000) / 100);
const millis = (micros: number) => String(micros / 1_000);

function typed(settings: Thresholds): Typed {
  return {
    error_warning: percent(settings.health.errorRateWarning),
    error_critical: percent(settings.health.errorRateCritical),
    p95_warning: millis(settings.health.p95WarningTime),
    p95_critical: millis(settings.health.p95CriticalTime),
    apdex: millis(settings.apdexThreshold),
    long_task: millis(settings.longTaskTime),
  };
}

/** The thresholds typed, or null while a field holds no number. */
function read(value: Typed): Thresholds | null {
  const numbers = Object.fromEntries(Object.entries(value).map(([key, text]) => [key, parseNumber(text)])) as Record<keyof Typed, number | null>;

  if (Object.values(numbers).some((number) => number === null)) return null;

  const number = (key: keyof Typed) => numbers[key] ?? 0;

  return {
    health: {
      errorRateWarning: number('error_warning') / 100,
      errorRateCritical: number('error_critical') / 100,
      p95WarningTime: Math.round(number('p95_warning') * 1_000),
      p95CriticalTime: Math.round(number('p95_critical') * 1_000),
    },
    apdexThreshold: Math.round(number('apdex') * 1_000),
    longTaskTime: Math.round(number('long_task') * 1_000),
  };
}

/**
 * From what error rate and P95 a service is a warning, then critical; the
 * Apdex threshold T of a service that sets none; and from what P99 an
 * endpoint is a long task.
 */
export function ThresholdsCard({
  saved,
  access,
  pending,
  onSave,
}: {
  saved: ApmSettings;
  access: ActionAccess;
  pending: boolean;
  onSave: (thresholds: Thresholds, done: () => void) => void;
}) {
  const { t } = useTranslation('apm');
  const draft = useDraft(typed(saved));
  const value = draft.value;
  const parsed = read(value);
  const issues = parsed ? thresholdIssues(parsed) : (['numbers'] as const);
  const bad = new Set<string>(issues);
  const set = (key: keyof Typed) => (text: string) => draft.set({ ...value, [key]: text });
  const disabled = access.disabled;

  return (
    <ApmCard
      icon={Gauge}
      tone="text-[var(--green-ink)]"
      title={t('org_settings.thresholds.title')}
      description={t('org_settings.thresholds.hint')}
      testId="settings-thresholds"
      actions={
        <CardSave
          name="thresholds"
          dirty={draft.dirty}
          invalid={!parsed || issues.length > 0}
          pending={pending}
          access={access}
          onSave={() => parsed && onSave(parsed, draft.reset)}
          onReset={draft.reset}
        />
      }
    >
      <div className="grid grid-cols-[120px_minmax(0,1fr)] items-center gap-x-3 gap-y-2 p-3 font-sans text-xs">
        <span className="font-strong text-tx-1">{t('org_settings.thresholds.error_rate')}</span>
        <span className="flex flex-wrap gap-x-4 gap-y-2">
          <NumberField label={t('org_settings.thresholds.warning')} unit="%" value={value.error_warning} onChange={set('error_warning')} invalid={bad.has('error_rates')} disabled={disabled} />
          <NumberField label={t('org_settings.thresholds.critical')} unit="%" value={value.error_critical} onChange={set('error_critical')} invalid={bad.has('error_rates')} disabled={disabled} />
        </span>
        <span className="font-strong text-tx-1">P95</span>
        <span className="flex flex-wrap gap-x-4 gap-y-2">
          <NumberField label={t('org_settings.thresholds.warning')} unit="ms" value={value.p95_warning} onChange={set('p95_warning')} invalid={bad.has('p95')} disabled={disabled} />
          <NumberField label={t('org_settings.thresholds.critical')} unit="ms" value={value.p95_critical} onChange={set('p95_critical')} invalid={bad.has('p95')} disabled={disabled} />
        </span>
        <span className="font-strong text-tx-1">{t('org_settings.thresholds.apdex')}</span>
        <span className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <NumberField label="T" unit="ms" value={value.apdex} onChange={set('apdex')} invalid={bad.has('apdex')} disabled={disabled} />
          <span className="text-tx-3">{t('org_settings.thresholds.apdex_hint')}</span>
        </span>
        <span className="font-strong text-tx-1">{t('org_settings.thresholds.long_task')}</span>
        <span className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <NumberField label="P99 ≥" unit="ms" value={value.long_task} onChange={set('long_task')} invalid={bad.has('long_task')} disabled={disabled} />
        </span>
      </div>
      <Issues issues={issues.map((issue) => t(`org_settings.thresholds.issues.${issue}`))} />
    </ApmCard>
  );
}
