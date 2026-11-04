import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as alertSourcesApi from '@/api/alertSources';
import type { AlertSource, MailboxSettings } from '@/api/alertSources';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput, FormRadio, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';

import { SECTION_INSET, SourceSection } from './Section';

const EMPTY: MailboxSettings = {
  host: '',
  port: 993,
  security: 'tls',
  username: '',
  auth: { kind: 'password' },
  folder: 'INBOX',
  pollSecs: 60,
};

/** The port a way of connecting usually takes: picking the way moves a port still at the other's default. */
const USUAL_PORT = { tls: 993, starttls: 143, none: 143 } as const;

/**
 * The IMAP mailbox an email source reads: the way that needs no public address, MX record or open
 * port. It is read only, starts from the moment it first connects, and is set up on the cluster that
 * opens the source's incidents, so its password stays there.
 */
export function MailboxSection({ source, elsewhere }: { source: AlertSource; elsewhere: boolean }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.sources.manage' });
  const queryKey = ['alert-sources', source.id, 'mailbox'];
  const [settings, setSettings] = React.useState<MailboxSettings>(EMPTY);
  const [secret, setSecret] = React.useState('');

  const query = useQuery({
    queryKey,
    queryFn: () => alertSourcesApi.mailbox(source.id),
    refetchInterval: 30_000,
  });

  const saved = query.data ?? null;

  React.useEffect(() => {
    setSettings(saved?.settings ?? EMPTY);
    setSecret('');
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only what was saved resets the form: a read every 30 seconds must not undo typing.
  }, [saved?.updatedAt]);

  const save = useMutation({
    mutationFn: () => alertSourcesApi.saveMailbox(source.id, { ...settings, secret }),
    onSuccess: () => {
      toast.success(t('alert_sources.mailbox.saved'));
      void queryClient.invalidateQueries({ queryKey });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const test = useMutation({
    mutationFn: () => alertSourcesApi.testMailbox(source.id, { ...settings, secret }),
    onSuccess: (status) => toast.success(t('alert_sources.mailbox.test_ok', { count: status.messages })),
    onError: (error) => toast.error(t('alert_sources.mailbox.test_failed', { error: toApiError(error).message })),
  });

  const remove = useMutation({
    mutationFn: () => alertSourcesApi.removeMailbox(source.id),
    onSuccess: () => {
      toast.success(t('alert_sources.mailbox.removed'));
      void queryClient.invalidateQueries({ queryKey });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const secretSet = saved?.secretSet ?? false;
  const oauth = settings.auth.kind === 'oauth2';
  const ready =
    settings.host.trim() !== '' &&
    settings.username.trim() !== '' &&
    (secret !== '' || secretSet) &&
    (settings.auth.kind !== 'oauth2' || (settings.auth.tokenUrl.trim() !== '' && settings.auth.clientId.trim() !== ''));

  const body = () => {
    if (elsewhere) {
      return (
        <p className={cn('pb-[20px] font-sans text-sm text-tx-2', SECTION_INSET)} data-mailbox-elsewhere>
          {t('alert_sources.mailbox.elsewhere', { cluster: source.homeCluster })}
        </p>
      );
    }

    return (
      <form
        className={cn('flex flex-col gap-3 pb-[20px] pt-1', SECTION_INSET)}
        data-mailbox-form
        onSubmit={(event) => {
          event.preventDefault();
          if (manageAccess.allowed && ready) save.mutate();
        }}
      >
        <fieldset disabled={manageAccess.disabled} aria-disabled={manageAccess.disabled || undefined} className="contents">
          <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_110px_150px]">
            <FormField label={t('alert_sources.mailbox.host')} required>
              <FormInput
                className="font-mono"
                value={settings.host}
                onChange={(event) => setSettings({ ...settings, host: event.target.value })}
                placeholder="imap.example.com"
              />
            </FormField>
            <FormField label={t('alert_sources.mailbox.port')} required>
              <FormInput
                type="number"
                min={1}
                max={65535}
                value={settings.port}
                onChange={(event) => setSettings({ ...settings, port: Number(event.target.value) })}
              />
            </FormField>
            <FormField label={t('alert_sources.mailbox.security')}>
              <FormSelect
                value={settings.security}
                onChange={(value) => {
                  const security = value as MailboxSettings['security'];
                  const moved = settings.port === USUAL_PORT[settings.security] ? USUAL_PORT[security] : settings.port;

                  setSettings({ ...settings, security, port: moved });
                }}
                options={(['tls', 'starttls', 'none'] as const).map((value) => ({
                  value,
                  label: t(`alert_sources.mailbox.security_${value}`),
                }))}
              />
            </FormField>
          </div>
          {settings.security === 'none' && (
            <p className="font-sans text-xs text-[var(--orange-ink)]" data-mailbox-plaintext>
              {t('alert_sources.mailbox.plaintext_warning')}
            </p>
          )}
          <FormRadio
            value={settings.auth.kind}
            onChange={(kind) =>
              setSettings({
                ...settings,
                auth: kind === 'oauth2' ? { kind, tokenUrl: '', clientId: '', scope: '' } : { kind: 'password' },
              })
            }
            className="grid grid-cols-2 gap-1.5"
            options={[
              {
                value: 'password' as const,
                label: t('alert_sources.mailbox.auth_password'),
                hint: t('alert_sources.mailbox.auth_password_hint'),
              },
              {
                value: 'oauth2' as const,
                label: t('alert_sources.mailbox.auth_oauth2'),
                hint: t('alert_sources.mailbox.auth_oauth2_hint'),
              },
            ]}
          />
          <div className="grid gap-2 sm:grid-cols-2">
            <FormField label={t('alert_sources.mailbox.username')} required>
              <FormInput
                value={settings.username}
                onChange={(event) => setSettings({ ...settings, username: event.target.value })}
                placeholder="alerts@example.com"
              />
            </FormField>
            <FormField
              label={t(oauth ? 'alert_sources.mailbox.client_secret' : 'alert_sources.mailbox.password')}
              required={!secretSet}
            >
              <FormInput
                type="password"
                autoComplete="new-password"
                value={secret}
                onChange={(event) => setSecret(event.target.value)}
                placeholder={secretSet ? t('alert_sources.mailbox.secret_kept') : ''}
              />
            </FormField>
          </div>
          {settings.auth.kind === 'oauth2' && (
            <div className="grid gap-2 sm:grid-cols-2">
              <FormField label={t('alert_sources.mailbox.token_url')} required>
                <FormInput
                  className="font-mono"
                  value={settings.auth.tokenUrl}
                  onChange={(event) =>
                    setSettings({ ...settings, auth: { ...(settings.auth as OAuth), tokenUrl: event.target.value } })
                  }
                  placeholder="https://login.microsoftonline.com/<tenant>/oauth2/v2.0/token"
                />
              </FormField>
              <FormField label={t('alert_sources.mailbox.client_id')} required>
                <FormInput
                  value={settings.auth.clientId}
                  onChange={(event) =>
                    setSettings({ ...settings, auth: { ...(settings.auth as OAuth), clientId: event.target.value } })
                  }
                />
              </FormField>
              <FormField label={t('alert_sources.mailbox.scope')}>
                <FormInput
                  className="font-mono"
                  value={settings.auth.scope}
                  onChange={(event) =>
                    setSettings({ ...settings, auth: { ...(settings.auth as OAuth), scope: event.target.value } })
                  }
                  placeholder="https://outlook.office365.com/.default"
                />
              </FormField>
            </div>
          )}
          <div className="grid gap-2 sm:grid-cols-2">
            <FormField label={t('alert_sources.mailbox.folder')}>
              <FormInput
                value={settings.folder}
                onChange={(event) => setSettings({ ...settings, folder: event.target.value })}
              />
            </FormField>
            <FormField label={t('alert_sources.mailbox.poll_secs')} hint={t('alert_sources.mailbox.poll_hint')}>
              <FormInput
                type="number"
                min={15}
                max={3600}
                value={settings.pollSecs}
                onChange={(event) => setSettings({ ...settings, pollSecs: Number(event.target.value) })}
              />
            </FormField>
          </div>
          {saved && (
            <p className="font-sans text-xs text-tx-2" data-mailbox-standing={saved.lastError ? 'failed' : 'fine'}>
              {saved.lastError ? (
                <span className="text-[var(--red-ink)]">
                  {t('alert_sources.mailbox.last_error', { error: saved.lastError })}
                </span>
              ) : saved.lastSuccessAt ? (
                t('alert_sources.mailbox.last_success', { time: formatMicrosActive(saved.lastSuccessAt, false) })
              ) : (
                t('alert_sources.mailbox.not_read_yet')
              )}
            </p>
          )}
          <div className="flex flex-wrap items-center gap-2">
            <ChromeButton
              type="submit"
              variant="primary"
              disabled={manageAccess.disabled || !ready || save.isPending}
              disabledReason={manageAccess.reason}
              data-mailbox-save
            >
              {t('alert_sources.mailbox.save')}
            </ChromeButton>
            <ChromeButton
              type="button"
              disabled={manageAccess.disabled || !ready || test.isPending}
              disabledReason={manageAccess.reason}
              onClick={() => test.mutate()}
              data-mailbox-test
            >
              {t('alert_sources.mailbox.test')}
            </ChromeButton>
            {saved && (
              <ChromeButton
                type="button"
                className="ml-auto"
                disabled={manageAccess.disabled || remove.isPending}
                disabledReason={manageAccess.reason}
                onClick={() => remove.mutate()}
                data-mailbox-remove
              >
                {t('alert_sources.mailbox.remove')}
              </ChromeButton>
            )}
          </div>
        </fieldset>
      </form>
    );
  };

  return (
    <SourceSection title={t('alert_sources.mailbox.title')} description={t('alert_sources.mailbox.description')}>
      {body()}
    </SourceSection>
  );
}

type OAuth = Extract<MailboxSettings['auth'], { kind: 'oauth2' }>;
