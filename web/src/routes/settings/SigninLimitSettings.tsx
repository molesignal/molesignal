import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { PageHeader } from '@/admin';
import * as instanceApi from '@/api/instance';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import { StatusWord } from '@/shell/StatusWord';
import { toast } from '@/shell/ui/sonner';

import {
  CopyableValue,
  ReadOnlyValue,
  SectionBody,
  SettingsGroupStack,
  SettingsRow,
  SettingsSection,
} from './_atoms';
import { useSettingsSaveStatus } from './SettingsSaveStatus';

/** Same bounds as the backend (`MAX_SIGNIN_LIMIT_*`). */
export const MAX_FAILURES = 100_000;
export const MAX_INTERVAL_SECS = 600;
export const MAX_LOCK_MINUTES = 1440;

const QUERY_KEY = ['settings', 'signin_limits'] as const;
const DRAFT_KEY = 'signin_limits';

interface Draft {
  accountSource: string;
  source: string;
  account: string;
  interval: string;
  lock: string;
}

function toDraft(limits: instanceApi.SigninLimitSettings): Draft {
  return {
    accountSource: String(limits.accountSourceFailures),
    source: String(limits.sourceFailures),
    account: String(limits.accountFailures),
    interval: String(limits.accountIntervalSecs),
    lock: String(limits.lockMinutes),
  };
}

function wholeNumber(value: string, min: number, max: number): number | null {
  const text = value.trim();
  if (!/^\d+$/.test(text)) return null;
  const number = Number(text);
  return number >= min && number <= max ? number : null;
}

/** The limits typed in the fields, or `null` when one is not a whole number within its bounds. */
export function parseSigninLimits(draft: Draft): instanceApi.SigninLimitSettings | null {
  const accountSource = wholeNumber(draft.accountSource, 0, MAX_FAILURES);
  const source = wholeNumber(draft.source, 0, MAX_FAILURES);
  const account = wholeNumber(draft.account, 0, MAX_FAILURES);
  const interval = wholeNumber(draft.interval, 1, MAX_INTERVAL_SECS);
  const lock = wholeNumber(draft.lock, 1, MAX_LOCK_MINUTES);

  if (
    accountSource === null ||
    source === null ||
    account === null ||
    interval === null ||
    lock === null
  ) {
    return null;
  }

  return {
    accountSourceFailures: accountSource,
    sourceFailures: source,
    accountFailures: account,
    accountIntervalSecs: interval,
    lockMinutes: lock,
  };
}

/**
 * What stops password guessing: how many failures lock an address out, and when a much-guessed
 * account starts to be slowed down. A platform setting, served in the platform organization only;
 * every sign-in reads it again.
 */
export function SigninLimitSettings() {
  const { t } = useTranslation('settings-admin');

  return (
    <>
      <PageHeader
        title={t('signin_limits.title')}
        subtitle={t('signin_limits.subtitle') as string}
      />
      <SectionBody opensOnSection className="pb-10">
        <SettingsGroupStack>
          <LimitsSection />
        </SettingsGroupStack>
      </SectionBody>
    </>
  );
}

function LimitsSection() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const access = useActionAccess({ permission: 'sys.settings.signin.manage' });
  const queryClient = useQueryClient();
  const saveStatus = useSettingsSaveStatus();
  const query = useQuery({
    queryKey: QUERY_KEY,
    queryFn: () => instanceApi.getSigninLimits(),
  });
  const [draft, setDraft] = React.useState<Draft | null>(null);

  React.useEffect(() => {
    if (query.data && draft === null) setDraft(toDraft(query.data));
  }, [draft, query.data]);

  const limits = draft === null ? null : parseSigninLimits(draft);
  const invalid = draft !== null && limits === null;
  const saved = query.data ? toDraft(query.data) : null;
  const dirty = Boolean(
    saved &&
      draft &&
      (Object.keys(saved) as (keyof Draft)[]).some((key) => draft[key].trim() !== saved[key]),
  );

  React.useEffect(() => {
    saveStatus.setDraftDirty(DRAFT_KEY, dirty);
  }, [dirty, saveStatus]);

  React.useEffect(
    () => () => saveStatus.setDraftDirty(DRAFT_KEY, false),
    [saveStatus],
  );

  const save = useMutation({
    mutationFn: (next: instanceApi.SigninLimitSettings) => instanceApi.updateSigninLimits(next),
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (view) => {
      queryClient.setQueryData(QUERY_KEY, view);
      setDraft(toDraft(view));
      saveStatus.completeSave();
      toast.success(t('signin_limits.saved'));
    },
    onError: (error) => {
      saveStatus.failSave();
      toast.error(toApiError(error).message);
    },
  });

  const controlsDisabled =
    access.disabled || query.isLoading || query.isError || save.isPending;
  const controlsDisabledReason =
    access.reason ??
    (query.isLoading
      ? tc('access.loading')
      : query.isError
        ? tc('access.page_unavailable')
        : save.isPending
          ? tc('access.operation_pending')
          : undefined);

  const reset = () => {
    if (query.data) setDraft(toDraft(query.data));
  };

  const persist = () => {
    if (limits === null || !dirty || controlsDisabled) return;
    save.mutate(limits);
  };

  const numberField = (key: keyof Draft, label: string, unit: string, min: number, max: number) => (
    <div className="flex items-center gap-2">
      <FormInput
        type="number"
        inputMode="numeric"
        min={min}
        max={max}
        step={1}
        value={draft?.[key] ?? ''}
        onChange={(event) =>
          setDraft((current) => (current ? { ...current, [key]: event.target.value } : current))
        }
        onKeyDown={(event) => {
          if (event.key === 'Enter') persist();
        }}
        disabled={controlsDisabled || draft === null}
        disabledReason={controlsDisabledReason}
        aria-label={label}
        aria-invalid={invalid || undefined}
        className="w-36 tabular-nums"
      />
      <span className="font-sans text-sm text-tx-2">{unit}</span>
    </div>
  );

  return (
    <>
      <SettingsSection
        title={t('signin_limits.section_title')}
        description={t('signin_limits.section_subtitle')}
      >
        <SettingsRow
          label={t('signin_limits.account_source')}
          description={t('signin_limits.account_source_hint')}
        >
          {numberField(
            'accountSource',
            t('signin_limits.account_source'),
            t('signin_limits.unit_failures'),
            0,
            MAX_FAILURES,
          )}
        </SettingsRow>
        <SettingsRow label={t('signin_limits.source')} description={t('signin_limits.source_hint')}>
          {numberField(
            'source',
            t('signin_limits.source'),
            t('signin_limits.unit_failures'),
            0,
            MAX_FAILURES,
          )}
        </SettingsRow>
        <SettingsRow
          label={t('signin_limits.account')}
          description={t('signin_limits.account_hint')}
        >
          {numberField(
            'account',
            t('signin_limits.account'),
            t('signin_limits.unit_failures'),
            0,
            MAX_FAILURES,
          )}
        </SettingsRow>
        <SettingsRow
          label={t('signin_limits.interval')}
          description={t('signin_limits.interval_hint')}
        >
          {numberField(
            'interval',
            t('signin_limits.interval'),
            t('signin_limits.unit_seconds'),
            1,
            MAX_INTERVAL_SECS,
          )}
        </SettingsRow>
        <SettingsRow label={t('signin_limits.lock')} description={t('signin_limits.lock_hint')}>
          {numberField(
            'lock',
            t('signin_limits.lock'),
            t('signin_limits.unit_minutes'),
            1,
            MAX_LOCK_MINUTES,
          )}
        </SettingsRow>

        <div className="flex min-h-16 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:justify-between">
          <div className="font-sans text-xs text-tx-3">
            {invalid
              ? t('signin_limits.invalid', {
                  failures: MAX_FAILURES,
                  seconds: MAX_INTERVAL_SECS,
                  minutes: MAX_LOCK_MINUTES,
                })
              : dirty
                ? t('signin_limits.unsaved')
                : t('signin_limits.synced')}
          </div>
          <div className="flex items-center justify-end gap-2">
            <ChromeButton
              onClick={reset}
              disabled={access.disabled || !dirty || save.isPending}
              disabledReason={
                access.reason ??
                (!dirty
                  ? tc('access.no_changes')
                  : save.isPending
                    ? tc('access.operation_pending')
                    : undefined)
              }
            >
              {tc('actions.reset')}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              onClick={persist}
              disabled={controlsDisabled || !dirty || limits === null}
              disabledReason={
                access.reason ??
                (invalid
                  ? tc('access.form_invalid')
                  : !dirty
                    ? tc('access.no_changes')
                    : controlsDisabledReason)
              }
            >
              {save.isPending ? t('signin_limits.saving') : t('signin_limits.save')}
            </ChromeButton>
          </div>
        </div>
      </SettingsSection>

      {query.data?.source && <SourceSection source={query.data.source} />}
    </>
  );
}

/** Says what the limits make of the reader's own request, the easiest way to see a proxy nobody set up. */
function SourceSection({ source }: { source: instanceApi.ObservedSigninSource }) {
  const { t } = useTranslation('settings-admin');
  const nav = useNavigate();

  const proxy = source.behindUnconfiguredProxy;

  const description = proxy
    ? source.counted
      ? t('signin_limits.source_proxy_public')
      : t('signin_limits.source_proxy_private')
    : source.counted
      ? t('signin_limits.source_counted')
      : source.address
        ? t('signin_limits.source_private')
        : t('signin_limits.source_none');

  return (
    <SettingsSection title={t('signin_limits.source_title')}>
      <SettingsRow
        label={t('signin_limits.source_label')}
        description={description}
        controlClassName="w-full"
      >
        <div className="flex w-full flex-col items-start gap-2" data-testid="signin-limits-source">
          {source.address ? (
            <CopyableValue
              value={source.address}
              copyLabel={t('signin_limits.copy_address')}
              copiedLabel={t('signin_limits.copied')}
            />
          ) : (
            <ReadOnlyValue value={t('signin_limits.source_unknown')} muted />
          )}
          <StatusWord tone={proxy ? 'yellow' : source.counted ? 'green' : 'neutral'}>
            {proxy
              ? t('signin_limits.state_proxy')
              : source.counted
                ? t('signin_limits.state_counted')
                : t('signin_limits.state_account_only')}
          </StatusWord>
          {(proxy || !source.counted) && (
            <ChromeButton onClick={() => nav('/settings/client_ip')}>
              {t('signin_limits.open_client_ip')}
            </ChromeButton>
          )}
        </div>
      </SettingsRow>
    </SettingsSection>
  );
}
