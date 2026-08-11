import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as statusPagesApi from '@/api/statusPages';
import type { StatusPageLanguage } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { FormChecklist, FormField, FormInput, FormRow, FormSelect } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import {
  MAX_STATUS_PAGE_HISTORY_DAYS,
  STATUS_PAGE_LANGUAGES,
  statusPageLanguages,
} from '../../model';
import { useStatusPageWorkspace } from '../Layout';
import { statusPageInput } from './pageInput';
import { SettingsCard, SettingsFooter } from './SettingsSection';

export function StatusPageLocalizationSettings() {
  const { t } = useTranslation('status-pages');
  const { pageId, snapshot, refresh } = useStatusPageWorkspace();
  const page = snapshot.page;
  const editable = page.lifecycle === 'active';
  const [timezone, setTimezone] = React.useState(page.timezone);
  const [language, setLanguage] = React.useState(page.language);
  const [languages, setLanguages] = React.useState(() => statusPageLanguages(page.language, page.languages));
  const [historyDays, setHistoryDays] = React.useState(String(page.history_days));
  const mutation = useMutation({
    mutationFn: () => statusPagesApi.update(pageId, statusPageInput(page, {
      timezone: timezone.trim(),
      language,
      languages,
      history_days: Number(historyDays),
    })),
    onSuccess: async () => {
      toast.success(t('toast.page_updated'));
      await refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const setAvailableLanguages = (next: StatusPageLanguage[]) => {
    if (next.length === 0) return;
    setLanguages(next);
    if (!next.includes(language)) setLanguage(next[0] ?? 'en-us');
  };
  const days = Number(historyDays);
  const invalid = !timezone.trim() || !Number.isInteger(days) || days < 1 || days > MAX_STATUS_PAGE_HISTORY_DAYS;

  return (
    <SettingsCard title={t('settings.localization.title')} description={t('settings.localization.description')}>
      <FormField label={t('fields.languages')}>
        <FormChecklist
          options={STATUS_PAGE_LANGUAGES}
          selected={languages}
          disabled={!editable}
          onChange={setAvailableLanguages}
          className="grid grid-cols-1 gap-2 sm:grid-cols-2"
        />
      </FormField>
      <FormRow className="grid-cols-1 sm:grid-cols-2">
        <FormField label={t('fields.default_language')}>
          <FormSelect
            value={language}
            disabled={!editable}
            onChange={(value) => setLanguage(value as StatusPageLanguage)}
            options={STATUS_PAGE_LANGUAGES.filter((option) => languages.includes(option.value))}
          />
        </FormField>
        <FormField label={t('fields.timezone')}>
          <FormInput disabled={!editable} value={timezone} onChange={(event) => setTimezone(event.currentTarget.value)} />
        </FormField>
      </FormRow>
      <FormField label={t('fields.history_days')} hint={t('forms.page.history_days_hint', { max: MAX_STATUS_PAGE_HISTORY_DAYS })}>
        <FormInput
          type="number"
          min={1}
          max={MAX_STATUS_PAGE_HISTORY_DAYS}
          value={historyDays}
          disabled={!editable}
          onChange={(event) => setHistoryDays(event.currentTarget.value)}
        />
      </FormField>
      <SettingsFooter>
        <ChromeButton variant="primary" disabled={!editable || invalid || mutation.isPending} onClick={() => mutation.mutate()}>
          {t('actions.save')}
        </ChromeButton>
      </SettingsFooter>
    </SettingsCard>
  );
}
