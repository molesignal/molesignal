import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';

import { FormField, FormSelect } from '@/shell/FormDrawer';

/** The delays offered, in seconds; `auto` leaves it to the deployment. */
const DELAYS = [0, 30, 60, 120, 300, 900, 1800, 3600] as const;

const AUTO = 'auto';

/** "1 min", "30 s", "1 h" for a delay in seconds. */
function delayLabel(seconds: number, t: TFunction<'alerts'>): string {
  if (seconds === 0) return t('workbench.delay.none');
  if (seconds % 3600 === 0) return t('workbench.delay.hours', { count: seconds / 3600 });
  if (seconds % 60 === 0) return t('workbench.delay.minutes', { count: seconds / 60 });
  return t('workbench.delay.seconds', { count: seconds });
}

/**
 * How far back each round's window is moved, to wait for data that arrives late or is not
 * written out yet. Left on auto, a single process waits for nothing and a cluster waits as long
 * as the platform setting says.
 */
export function DelayField({
  value,
  onChange,
}: {
  value: number | null;
  onChange: (value: number | null) => void;
}) {
  const { t } = useTranslation('alerts');
  const known = value === null || (DELAYS as readonly number[]).includes(value);

  return (
    <FormField label={t('workbench.fields.delay')} hint={t('workbench.hints.delay')}>
      <FormSelect
        value={value === null ? AUTO : String(value)}
        onChange={(next) => onChange(next === AUTO ? null : Number(next))}
        options={[
          { value: AUTO, label: t('workbench.delay.auto') },
          ...DELAYS.map((seconds) => ({ value: String(seconds), label: delayLabel(seconds, t) })),
          ...(known || value === null ? [] : [{ value: String(value), label: delayLabel(value, t) }]),
        ]}
      />
    </FormField>
  );
}
