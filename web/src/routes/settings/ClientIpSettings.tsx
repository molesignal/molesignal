import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { PageHeader } from '@/admin';
import * as instanceApi from '@/api/instance';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
import {
  FormInput,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import {
  SectionBody,
  SettingsGroupStack,
  SettingsRow,
  SettingsSection,
} from './_atoms';
import { useSettingsSaveStatus } from './SettingsSaveStatus';

const HEADER_NAME_PATTERN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
const SENSITIVE_HEADERS = new Set([
  'authorization',
  'proxy-authorization',
  'cookie',
  'set-cookie',
  'x-api-key',
  'x-auth-token',
  'x-forwarded-client-cert',
]);

interface Draft {
  mode: instanceApi.ClientIpMode;
  headerName: string;
  trustedCidrs: string;
  fallbackToPeer: boolean;
  allowPrivateClientIps: boolean;
  maxChainLength: string;
}

function draftFromSettings(settings: instanceApi.ClientIpResolverSettings): Draft {
  return {
    mode: settings.mode,
    headerName: settings.header_name,
    trustedCidrs: settings.trusted_proxy_cidrs.join('\n'),
    fallbackToPeer: settings.fallback_to_peer,
    allowPrivateClientIps: settings.allow_private_client_ips,
    maxChainLength: String(settings.max_chain_length),
  };
}

function settingsFromDraft(draft: Draft): instanceApi.ClientIpResolverSettings {
  const usesProxy = draft.mode !== 'peer';
  return {
    mode: draft.mode,
    header_name: usesProxy ? draft.headerName.trim().toLowerCase() : '',
    trusted_proxy_cidrs: usesProxy
      ? draft.trustedCidrs
          .split(/\r?\n/)
          .map((value) => value.trim())
          .filter(Boolean)
      : [],
    fallback_to_peer: draft.fallbackToPeer,
    allow_private_client_ips: draft.allowPrivateClientIps,
    max_chain_length: Number(draft.maxChainLength),
  };
}

function sameSettings(
  left: instanceApi.ClientIpResolverSettings,
  right: instanceApi.ClientIpResolverSettings,
) {
  return (
    left.mode === right.mode &&
    left.header_name === right.header_name &&
    left.fallback_to_peer === right.fallback_to_peer &&
    left.allow_private_client_ips === right.allow_private_client_ips &&
    left.max_chain_length === right.max_chain_length &&
    left.trusted_proxy_cidrs.length === right.trusted_proxy_cidrs.length &&
    left.trusted_proxy_cidrs.every(
      (cidr, index) => cidr === right.trusted_proxy_cidrs[index],
    )
  );
}

function validateDraft(draft: Draft): string | null {
  const maxChainLength = Number(draft.maxChainLength);
  if (
    !Number.isInteger(maxChainLength) ||
    maxChainLength < 1 ||
    maxChainLength > 64
  ) {
    return 'chain_length';
  }
  if (draft.mode === 'peer') return null;

  const headerName = draft.headerName.trim();
  if (
    headerName.length < 1 ||
    headerName.length > 128 ||
    !HEADER_NAME_PATTERN.test(headerName) ||
    SENSITIVE_HEADERS.has(headerName.toLowerCase())
  ) {
    return 'header';
  }
  const cidrs = draft.trustedCidrs
    .split(/\r?\n/)
    .map((value) => value.trim())
    .filter(Boolean);
  if (
    cidrs.length < 1 ||
    cidrs.length > 64 ||
    cidrs.some((cidr) => !/^\S+\/\d{1,3}$/.test(cidr))
  ) {
    return 'cidrs';
  }
  return null;
}

export function ClientIpSettings() {
  const { t } = useTranslation('settings-admin');
  const access = useActionAccess({ permission: 'sys.settings.manage' });

  return (
    <>
      <PageHeader
        title={t('nodes.client_ip.title')}
        subtitle={t('nodes.client_ip.subtitle') as string}
      />
      <SectionBody className="pb-10">
        <SettingsGroupStack>
          <ClientIpSettingsSection access={access} />
        </SettingsGroupStack>
      </SectionBody>
    </>
  );
}

function ClientIpSettingsSection({ access }: { access: ActionAccess }) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const saveStatus = useSettingsSaveStatus();
  const query = useQuery({
    queryKey: ['settings', 'client_ip'],
    queryFn: () => instanceApi.getClientIpSettings(),
  });
  const [draft, setDraft] = React.useState<Draft | null>(null);

  React.useEffect(() => {
    if (query.data && draft === null) {
      setDraft(draftFromSettings(query.data));
    }
  }, [draft, query.data]);

  const current = draft ? settingsFromDraft(draft) : null;
  const dirty = Boolean(
    current && query.data && !sameSettings(current, query.data),
  );
  const validationError = draft ? validateDraft(draft) : null;
  const usesProxy = draft ? draft.mode !== 'peer' : false;
  const usesChain = draft?.mode === 'forwarded_chain';

  React.useEffect(() => {
    saveStatus.setDraftDirty('nodes.client_ip', dirty);
  }, [dirty, saveStatus]);

  React.useEffect(
    () => () => saveStatus.setDraftDirty('nodes.client_ip', false),
    [saveStatus],
  );

  const save = useMutation({
    mutationFn: (next: instanceApi.ClientIpResolverSettings) =>
      instanceApi.updateClientIpSettings(next),
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (saved) => {
      queryClient.setQueryData(['settings', 'client_ip'], saved);
      setDraft(draftFromSettings(saved));
      saveStatus.completeSave();
      toast.success(t('nodes.client_ip.saved'));
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

  const reset = React.useCallback(() => {
    if (query.data) setDraft(draftFromSettings(query.data));
  }, [query.data]);

  const persist = React.useCallback(() => {
    if (!current || !dirty || validationError || controlsDisabled) return;
    save.mutate(current);
  }, [controlsDisabled, current, dirty, save, validationError]);

  const set = <K extends keyof Draft>(key: K, value: Draft[K]) => {
    setDraft((previous) => (previous ? { ...previous, [key]: value } : previous));
  };

  return (
    <SettingsSection
      title={t('nodes.client_ip.resolver_title')}
      description={t('nodes.client_ip.resolver_subtitle')}
    >
      <SettingsRow
        label={t('nodes.client_ip.mode')}
        description={t(`nodes.client_ip.mode_hints.${draft?.mode ?? 'peer'}`)}
        controlClassName="w-full"
      >
        <div className="flex w-full flex-col gap-2">
          <FormSelect
            value={draft?.mode ?? 'peer'}
            onChange={(value) =>
              set('mode', value as instanceApi.ClientIpMode)
            }
            options={[
              { value: 'peer', label: t('nodes.client_ip.modes.peer') },
              { value: 'header', label: t('nodes.client_ip.modes.header') },
              {
                value: 'forwarded_chain',
                label: t('nodes.client_ip.modes.forwarded_chain'),
              },
            ]}
            disabled={controlsDisabled || draft === null}
            disabledReason={controlsDisabledReason}
            ariaLabel={t('nodes.client_ip.mode')}
          />
          <Pill tone={usesProxy ? 'yellow' : 'green'}>
            {usesProxy
              ? t('nodes.client_ip.trust_required')
              : t('nodes.client_ip.no_headers')}
          </Pill>
        </div>
      </SettingsRow>

      {usesProxy && draft && (
        <>
          <SettingsRow
            label={t('nodes.client_ip.header_name')}
            description={t('nodes.client_ip.header_hint')}
            controlClassName="w-full"
          >
            <FormInput
              value={draft.headerName}
              onChange={(event) => set('headerName', event.target.value)}
              placeholder={
                usesChain ? 'X-Forwarded-For' : 'CF-Connecting-IP'
              }
              autoComplete="off"
              disabled={controlsDisabled}
              disabledReason={controlsDisabledReason}
              aria-label={t('nodes.client_ip.header_name')}
            />
          </SettingsRow>

          <SettingsRow
            label={t('nodes.client_ip.trusted_cidrs')}
            description={t('nodes.client_ip.trusted_cidrs_hint')}
            controlClassName="w-full"
          >
            <FormTextarea
              value={draft.trustedCidrs}
              onChange={(event) => set('trustedCidrs', event.target.value)}
              placeholder={'10.0.0.0/8\n2001:db8:1::/48'}
              rows={4}
              spellCheck={false}
              disabled={controlsDisabled}
              disabledReason={controlsDisabledReason}
              aria-label={t('nodes.client_ip.trusted_cidrs')}
            />
          </SettingsRow>

          {usesChain && (
            <SettingsRow
              label={t('nodes.client_ip.chain_strategy')}
              description={t('nodes.client_ip.chain_strategy_hint')}
              controlClassName="w-full"
            >
              <div className="grid w-full grid-cols-1 gap-3 sm:grid-cols-2">
                <FormInput
                  value="rightmost_untrusted"
                  readOnly
                  aria-label={t('nodes.client_ip.chain_strategy')}
                />
                <FormInput
                  type="number"
                  min={1}
                  max={64}
                  value={draft.maxChainLength}
                  onChange={(event) =>
                    set('maxChainLength', event.target.value)
                  }
                  disabled={controlsDisabled}
                  disabledReason={controlsDisabledReason}
                  aria-label={t('nodes.client_ip.max_chain_length')}
                />
              </div>
            </SettingsRow>
          )}

          <SettingsRow
            label={t('nodes.client_ip.fallback')}
            description={t('nodes.client_ip.fallback_hint')}
          >
            <Switch
              checked={draft.fallbackToPeer}
              onCheckedChange={(checked) => set('fallbackToPeer', checked)}
              disabled={controlsDisabled}
              disabledReason={controlsDisabledReason}
              aria-label={t('nodes.client_ip.fallback')}
            />
          </SettingsRow>

          <SettingsRow
            label={t('nodes.client_ip.allow_private')}
            description={t('nodes.client_ip.allow_private_hint')}
          >
            <Switch
              checked={draft.allowPrivateClientIps}
              onCheckedChange={(checked) =>
                set('allowPrivateClientIps', checked)
              }
              disabled={controlsDisabled}
              disabledReason={controlsDisabledReason}
              aria-label={t('nodes.client_ip.allow_private')}
            />
          </SettingsRow>
        </>
      )}

      <div className="flex min-h-16 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="font-sans text-xs text-tx-3">
          {validationError
            ? t(`nodes.client_ip.validation.${validationError}`)
            : dirty
              ? t('nodes.client_ip.unsaved')
              : t('nodes.client_ip.synced')}
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
            disabled={
              controlsDisabled || !dirty || validationError !== null || !current
            }
            disabledReason={
              access.reason ??
              (validationError
                ? tc('access.form_invalid')
                : !dirty
                  ? tc('access.no_changes')
                  : controlsDisabledReason)
            }
          >
            {save.isPending
              ? t('nodes.client_ip.saving')
              : t('nodes.client_ip.save')}
          </ChromeButton>
        </div>
      </div>
    </SettingsSection>
  );
}
