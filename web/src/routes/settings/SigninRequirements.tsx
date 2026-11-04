import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, PageHeader } from '@/admin';
import * as mfaApi from '@/api/mfa';
import type * as ssoApi from '@/api/sso';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { useAuthStore } from '@/stores/auth';

import { SectionBody, SettingsGroupStack } from './_atoms';
import { SignupPolicySettings } from './SignupPolicySettings';
import { formatMicros } from '../rum/_helpers';
import { enforcementCheck, standingOf } from './SsoProviders/signinTest/model';

const QUERY_KEY = ['settings', 'signin-policy'] as const;

const NO_PROVIDERS: readonly ssoApi.SsoProvider[] = [];

function RequirementRow({
  label,
  description,
  note,
  checked,
  disabled,
  onChange,
  testId,
}: {
  label: string;
  description: string;
  /** Why it can't be turned on just now. */
  note?: string | undefined;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
  testId: string;
}) {
  return (
    <div
      className="grid gap-3 py-2.5 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center"
      data-testid={testId}
    >
      <div className="min-w-0">
        <div className="font-sans text-sm font-strong text-tx-0">{label}</div>
        <div className="mt-1 max-w-2xl font-sans text-xs leading-relaxed text-tx-2">
          {description}
        </div>
        {note && <div className="mt-1 font-sans text-xs leading-relaxed text-tx-2">{note}</div>}
      </div>
      <Switch checked={checked} disabled={disabled} aria-label={label} onCheckedChange={onChange} />
    </div>
  );
}

/**
 * What entering this organization asks of a sign-in. Turning a requirement on signs
 * out the sessions that don't meet it, so it asks first; turning one off doesn't.
 */
export function SigninRequirementRows({
  permission,
  ssoChoice,
  ssoProviders = NO_PROVIDERS,
}: {
  permission: 'org.signin.manage' | 'sys.settings.signin.manage';
  /** The platform organization has no single sign-on of its own to require. */
  ssoChoice: boolean;
  /** The organization's enabled OIDC, OAuth 2.0 and SAML providers. */
  ssoProviders?: readonly ssoApi.SsoProvider[];
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission });
  const organizationName = useAuthStore((state) => state.ctx?.orgName);
  const [confirming, setConfirming] = React.useState<'requireMfa' | 'ssoOnly' | null>(null);

  const query = useQuery({ queryKey: QUERY_KEY, queryFn: () => mfaApi.signinPolicy() });
  const policy = query.data;

  const save = useMutation({
    mutationFn: (next: mfaApi.SigninPolicy) => mfaApi.updateSigninPolicy(next),
    onSuccess: (saved) => {
      queryClient.setQueryData(QUERY_KEY, saved);
      setConfirming(null);
      toast.success(t('signin_requirements.saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const change = (key: 'requireMfa' | 'ssoOnly', checked: boolean) => {
    if (!policy) return;

    if (checked) setConfirming(key);
    else save.mutate({ ...policy, [key]: false });
  };

  if (query.isError) {
    return (
      <div className="py-2.5 font-sans text-xs text-red-soft">
        {toApiError(query.error).message}
      </div>
    );
  }

  const disabled = !policy || access.disabled || save.isPending;
  const ssoAvailable = ssoProviders.length > 0;
  const { proven, unproven } = enforcementCheck(ssoProviders);
  // With no provider that has shown it signs in, this can lock everyone out: it takes more to do.
  const risky = confirming === 'ssoOnly' && proven.length === 0;
  const ssoNote = !ssoAvailable
    ? policy?.ssoOnly
      ? undefined
      : t('signin_requirements.sso_unavailable')
    : proven.length === 0
      ? t('signin_requirements.sso_untested_note')
      : undefined;

  const confirmFacts = () => {
    if (confirming !== 'ssoOnly') return undefined;

    if (!risky) {
      return [
        {
          label: t('signin_requirements.fact_tested'),
          value: proven.map((provider) => (
            <div key={provider.id}>
              {t('signin_requirements.fact_tested_value', {
                name: provider.name,
                when: formatMicros(standingOf(provider).finishedAt),
              })}
            </div>
          )),
        },
      ];
    }

    return [
      {
        label: t('signin_requirements.fact_providers'),
        value: unproven.map((provider) => (
          <div key={provider.id}>
            {t('signin_requirements.fact_provider_value', {
              name: provider.name,
              status: t(`sso_providers.test.standing.${standingOf(provider).status}`),
            })}
          </div>
        )),
      },
      {
        label: t('signin_requirements.fact_sessions'),
        value: t('signin_requirements.fact_sessions_value'),
      },
      {
        label: t('signin_requirements.fact_clients'),
        value: t('signin_requirements.fact_clients_value'),
      },
      {
        label: t('signin_requirements.fact_way_back'),
        value: t('signin_requirements.fact_way_back_value'),
      },
    ];
  };

  const confirmTitle = !confirming
    ? ''
    : confirming === 'requireMfa'
      ? t('signin_requirements.mfa_confirm_title')
      : risky
        ? t('signin_requirements.sso_untested_title')
        : t('signin_requirements.sso_confirm_title');

  const confirmDescription = !confirming
    ? ''
    : confirming === 'requireMfa'
      ? t('signin_requirements.mfa_confirm_description')
      : risky
        ? t('signin_requirements.sso_untested_description')
        : t('signin_requirements.sso_confirm_description');

  return (
    <div className="flex min-w-0 flex-col">
      <RequirementRow
        testId="signin-requirement-mfa"
        label={t('signin_requirements.mfa_label')}
        description={t(
          ssoChoice ? 'signin_requirements.mfa_description' : 'signin_requirements.mfa_platform',
        )}
        checked={policy?.requireMfa ?? false}
        disabled={disabled}
        onChange={(checked) => change('requireMfa', checked)}
      />
      {ssoChoice && (
        <RequirementRow
          testId="signin-requirement-sso"
          label={t('signin_requirements.sso_label')}
          description={t('signin_requirements.sso_description')}
          note={ssoNote}
          checked={policy?.ssoOnly ?? false}
          disabled={disabled || (!ssoAvailable && !policy?.ssoOnly)}
          onChange={(checked) => change('ssoOnly', checked)}
        />
      )}
      <ConfirmDialog
        open={confirming !== null}
        onOpenChange={(open) => !open && setConfirming(null)}
        title={confirmTitle}
        description={confirmDescription}
        consequences={confirmFacts()}
        typeToConfirm={risky ? organizationName : undefined}
        destructive={risky}
        confirmLabel={t(risky ? 'signin_requirements.confirm_untested' : 'signin_requirements.confirm')}
        cancelLabel={tc('actions.cancel')}
        busy={save.isPending}
        onConfirm={() => {
          if (policy && confirming) save.mutate({ ...policy, [confirming]: true });
        }}
      />
    </div>
  );
}

/** The platform organization's page: only the platform administrator enters it. */
export function SigninRequirementsSettings() {
  const { t } = useTranslation('settings-admin');

  return (
    <>
      <PageHeader
        title={t('signin_requirements.title')}
        subtitle={t('signin_requirements.platform_subtitle') as string}
      />
      <SectionBody opensOnSection className="pb-10">
        <SettingsGroupStack>
          <SigninRequirementRows permission="sys.settings.signin.manage" ssoChoice={false} />
          <SignupPolicySettings />
        </SettingsGroupStack>
      </SectionBody>
    </>
  );
}
