import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';

import type { CountingOutcome } from './evaluate';

/**
 * Under a counting step in the test: what it would count the sample as, and
 * what it would do once its limit is reached. The test itself counts nothing.
 */
export function CountingNote({ outcome }: { outcome: CountingOutcome }) {
  const { t } = useTranslation('pipelines');

  return (
    <span data-counting-note={outcome.kind} className="mt-[2px] block leading-relaxed text-tx-3">
      {line(outcome, t)}
    </span>
  );
}

type Translate = TFunction<'pipelines'>;

function line(outcome: CountingOutcome, t: Translate): string {
  if (outcome.kind === 'not_picked') return t('counting.test.not_picked');

  if (outcome.kind === 'metric') {
    if (outcome.value === null) return t('counting.test.no_number', { field: outcome.field });

    return t(`counting.test.metric_${outcome.type}`, {
      series: outcome.series,
      value: outcome.value.toLocaleString(),
    });
  }

  const bucket =
    outcome.bucket.length === 0
      ? t(`counting.test.whole_${outcome.step}`)
      : outcome.bucket.map(([field, value]) => `${field}=${value ?? '∅'}`).join(' · ');
  const past =
    outcome.action === 'route'
      ? t('counting.test.past_route', { stream: outcome.target })
      : t(`counting.test.past_${outcome.action}`);
  const limit =
    typeof outcome.per === 'number'
      ? t('counting.test.throttle_limit', { limit: outcome.limit.toLocaleString(), window: outcome.per })
      : t('counting.test.quota_limit', {
          limit: outcome.limit.toLocaleString(),
          unit: t(`counting.units.${outcome.per}`),
        });

  return t('counting.test.limit', { bucket, limit, past });
}
