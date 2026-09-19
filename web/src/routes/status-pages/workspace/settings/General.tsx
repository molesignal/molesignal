import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as statusPagesApi from '@/api/statusPages';
import type { StatusPageVisibility } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput, FormRow, FormSelect } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { useStatusPageWorkspace } from '../Layout';
import { statusPageInput } from './pageInput';
import { SettingsCard, SettingsFooter } from './SettingsSection';

export function StatusPageGeneralSettings() {
  const { t } = useTranslation('status-pages');
  const { pageId, snapshot, refresh } = useStatusPageWorkspace();
  const page = snapshot.page;
  const editable = page.lifecycle === 'active';
  const [name, setName] = React.useState(page.name);
  const [slug, setSlug] = React.useState(page.slug);
  const [visibility, setVisibility] = React.useState<StatusPageVisibility>(page.visibility);
  const [deliveryRetention, setDeliveryRetention] = React.useState(String(page.delivery_retention_days));
  const [privateSessionDays, setPrivateSessionDays] = React.useState(String(page.private_session_days));
  React.useEffect(() => {
    setName(page.name);
    setSlug(page.slug);
    setVisibility(page.visibility);
    setDeliveryRetention(String(page.delivery_retention_days));
    setPrivateSessionDays(String(page.private_session_days));
  }, [page]);
  const mutation = useMutation({
    mutationFn: () =>
      statusPagesApi.update(pageId, statusPageInput(page, {
        name: name.trim(),
        slug: slug.trim(),
        visibility,
        delivery_retention_days: Number(deliveryRetention),
        private_session_days: Number(privateSessionDays),
      })),
    onSuccess: async () => {
      toast.success(t('toast.page_updated'));
      await refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const invalid = !name.trim() || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(slug);

  return (
    <SettingsCard title={t('settings.general.title')} description={t('settings.general.description')}>
      <FormRow className="grid-cols-1 sm:grid-cols-2">
        <FormField label={t('fields.name')} required>
          <FormInput disabled={!editable} value={name} maxLength={120} onChange={(event) => setName(event.currentTarget.value)} />
        </FormField>
        <FormField label={t('fields.slug')} required>
          <FormInput disabled={!editable} value={slug} maxLength={64} onChange={(event) => setSlug(event.currentTarget.value.toLowerCase())} />
        </FormField>
      </FormRow>
      <FormField label={t('fields.visibility')}>
        <FormSelect
          value={visibility}
          disabled={!editable}
          onChange={(value) => setVisibility(value as StatusPageVisibility)}
          options={[
            { value: 'public', label: t('visibility.public') },
            { value: 'private', label: t('visibility.private') },
          ]}
        />
      </FormField>
      <FormRow className="grid-cols-1 sm:grid-cols-2">
        <FormField label={t('fields.delivery_retention_days')}>
          <FormSelect
            value={deliveryRetention}
            disabled={!editable}
            onChange={setDeliveryRetention}
            options={[30, 60, 90, 180, 365].map((days) => ({ value: String(days), label: t('values.days', { count: days }) }))}
          />
        </FormField>
        <FormField label={t('fields.private_session_days')}>
          <FormSelect
            value={privateSessionDays}
            disabled={!editable}
            onChange={setPrivateSessionDays}
            options={[1, 7, 30].map((days) => ({ value: String(days), label: t('values.days', { count: days }) }))}
          />
        </FormField>
      </FormRow>
      <SettingsFooter>
        <ChromeButton variant="primary" disabled={!editable || invalid || mutation.isPending} onClick={() => mutation.mutate()}>
          {t('actions.save')}
        </ChromeButton>
      </SettingsFooter>
    </SettingsCard>
  );
}
