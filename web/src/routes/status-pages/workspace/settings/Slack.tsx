import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { Trans, useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as statusPagesApi from '@/api/statusPages';
import type { StatusPageSlackAppSetting } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { CopyField } from '@/shell/CopyField';
import { FormField, FormSelect } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { useStatusPageWorkspace } from '../Layout';
import { SettingsCard, SettingsFooter } from './SettingsSection';

const CONNECTORS_PATH = '/settings/notify/connectors';

/** "Add to Slack": the Slack App connector visitors add the page to their channels with. */
export function StatusPageSlackSettings() {
  const { t } = useTranslation('status-pages');
  const { pageId, snapshot } = useStatusPageWorkspace();
  const setting = useQuery({
    queryKey: ['status-pages', pageId, 'slack-app'],
    queryFn: () => statusPagesApi.getSlackAppSetting(pageId),
  });

  return (
    <SettingsCard title={t('settings.slack.title')} description={t('settings.slack.description')}>
      {snapshot.page.visibility === 'private' && (
        <p className="text-xs leading-5 text-tx-3">{t('settings.slack.private_page')}</p>
      )}
      {setting.data ? (
        // The form starts from the setting it is given, so its select never
        // receives a value before the option that holds it.
        <SlackAppForm pageId={pageId} setting={setting.data} editable={snapshot.page.lifecycle === 'active'} />
      ) : setting.isError ? (
        <p className="text-xs leading-5 text-red">{toApiError(setting.error).message}</p>
      ) : (
        <p className="text-xs leading-5 text-tx-3">{t('settings.slack.loading')}</p>
      )}
    </SettingsCard>
  );
}

function SlackAppForm({
  pageId,
  setting,
  editable,
}: {
  pageId: string;
  setting: StatusPageSlackAppSetting;
  editable: boolean;
}) {
  const { t } = useTranslation('status-pages');
  const queryClient = useQueryClient();
  const [connectorId, setConnectorId] = React.useState(setting.connector_id ?? '');
  const mutation = useMutation({
    mutationFn: () => statusPagesApi.setSlackApp(pageId, connectorId || null),
    onSuccess: (next) => {
      queryClient.setQueryData(['status-pages', pageId, 'slack-app'], next);
      toast.success(t(next.connector_id ? 'toast.slack_app_on' : 'toast.slack_app_off'));
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const options = [
    { value: '', label: t('settings.slack.off') },
    ...setting.connectors.map((connector) => ({
      value: connector.id,
      label: connector.name,
      disabled: !connector.enabled || !connector.adds_channels,
      disabledReason: !connector.enabled
        ? t('settings.slack.connector_off')
        : t('settings.slack.connector_without_client'),
    })),
  ];

  return (
    <>
      <FormField label={t('fields.slack_app')}>
        <FormSelect value={connectorId} disabled={!editable} onChange={setConnectorId} options={options} />
      </FormField>
      {setting.connectors.length === 0 && (
        <p className="text-xs leading-5 text-tx-3">
          <Trans
            t={t}
            i18nKey="settings.slack.no_connectors"
            components={{ connectors: <Link className="text-tx-1 underline underline-offset-2" to={CONNECTORS_PATH} /> }}
          />
        </p>
      )}
      {/* Not a <label>: a click on its text would press the copy button. */}
      <div className="grid min-w-0 gap-1.5">
        <span className={uiLabelClass}>{t('fields.slack_redirect_url')}</span>
        {setting.redirect_url ? (
          <CopyField value={setting.redirect_url} copyLabel={t('actions.copy')} />
        ) : (
          <p className="text-xs leading-5 text-yellow">{t('settings.slack.needs_https')}</p>
        )}
        <span className="text-xs leading-relaxed text-tx-3">{t('settings.slack.redirect_hint')}</span>
      </div>
      <SettingsFooter>
        <ChromeButton
          variant="primary"
          disabled={!editable || connectorId === (setting.connector_id ?? '') || mutation.isPending}
          onClick={() => mutation.mutate()}
        >
          {t('actions.save')}
        </ChromeButton>
      </SettingsFooter>
    </>
  );
}
