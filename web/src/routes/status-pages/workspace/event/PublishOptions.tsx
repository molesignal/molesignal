import { useTranslation } from 'react-i18next';

import type { PublicIncidentStatus, StatusPageIncidentKind } from '@/api/statusPages';
import { FormField, FormSelect } from '@/shell/FormDrawer';

import { NotifySwitch } from '../NotifySwitch';
import { NEXT_UPDATE_CHOICES, promisesNextUpdate } from './draft';

/**
 * What goes out with an update besides its words: by when the next one is
 * promised, and whether subscribers are told of this one.
 */
export function PublishOptions({
  kind,
  status,
  nextUpdateMinutes,
  onNextUpdateChange,
  notify,
  onNotifyChange,
  disabled,
}: {
  kind: StatusPageIncidentKind;
  status: PublicIncidentStatus;
  nextUpdateMinutes: number | null;
  onNextUpdateChange: (minutes: number | null) => void;
  notify: boolean;
  onNotifyChange: (notify: boolean) => void;
  disabled?: boolean;
}) {
  const { t } = useTranslation('status-pages');

  return (
    <>
      {promisesNextUpdate(kind, status) && (
        <FormField label={t('fields.next_update')} hint={t('forms.update.next_update_hint')}>
          <FormSelect
            value={nextUpdateMinutes === null ? '' : String(nextUpdateMinutes)}
            disabled={Boolean(disabled)}
            onChange={(value) => onNextUpdateChange(value ? Number(value) : null)}
            options={[
              { value: '', label: t('forms.update.next_update_none') },
              ...NEXT_UPDATE_CHOICES.map((minutes) => ({
                value: String(minutes),
                label: minutes < 60
                  ? t('forms.update.next_update_minutes', { count: minutes })
                  : t('forms.update.next_update_hours', { count: minutes / 60 }),
              })),
            ]}
          />
        </FormField>
      )}
      <NotifySwitch
        label={t('fields.notify_subscribers')}
        hint={t(notify ? 'forms.update.notify_hint' : 'forms.update.silent_hint')}
        checked={notify}
        onCheckedChange={onNotifyChange}
        disabled={Boolean(disabled)}
      />
    </>
  );
}
