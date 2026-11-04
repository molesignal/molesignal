import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { MfaMethod } from '@/api/auth';
import * as mfaApi from '@/api/mfa';
import { useTimeFormatter } from '@/lib/time';
import { passkeysSupported } from '@/lib/webauthn';
import { ChromeButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';

import { SecurityRow } from '../SecurityRow';
import { useCodeErrorToast } from './CodeField';
import { PasskeySection } from './PasskeySection';
import { ProofDialog } from './ProofDialog';
import { RecoveryCodesDialog } from './RecoveryCodesDialog';
import { SetupDialog } from './SetupDialog';

export const MFA_QUERY_KEY = ['me', 'mfa'] as const;

/** What the account can confirm a change with: what asks least comes first. */
function factorMethods(status: mfaApi.MfaStatus | undefined): MfaMethod[] {
  if (!status) return [];

  const methods: MfaMethod[] = [];

  if (status.passkeys.length > 0 && !status.passkeysUnavailable && passkeysSupported()) {
    methods.push('passkey');
  }

  if (status.totpEnabled) methods.push('totp');
  methods.push('recovery_code');

  return methods;
}

/**
 * Multi-factor authentication on the security page: the authenticator app, the
 * passkeys, and while either is on, the recovery codes. Turning it on or off,
 * replacing the authenticator or removing a second factor signs out the account's
 * other devices; this browser stays.
 */
export function MfaSection({ email }: { email: string }) {
  const { t } = useTranslation('account');
  const time = useTimeFormatter();
  const queryClient = useQueryClient();
  const codeError = useCodeErrorToast();
  const [confirming, setConfirming] = React.useState<'enable' | 'replace' | null>(null);
  const [replacing, setReplacing] = React.useState(false);
  const [setup, setSetup] = React.useState<mfaApi.TotpSetup | null>(null);
  const [disabling, setDisabling] = React.useState(false);
  const [regenerating, setRegenerating] = React.useState(false);
  const [failures, setFailures] = React.useState(0);
  const [codes, setCodes] = React.useState<string[] | null>(null);

  const statusQuery = useQuery({ queryKey: MFA_QUERY_KEY, queryFn: () => mfaApi.status() });
  const refresh = () => queryClient.invalidateQueries({ queryKey: MFA_QUERY_KEY });

  const failed = (error: unknown) => {
    setFailures((count) => count + 1);
    codeError(error);
  };

  const start = useMutation({
    mutationFn: (proof?: mfaApi.Proof) => mfaApi.setupTotp(proof),
    onSuccess: (next) => {
      setConfirming(null);
      setSetup(next);
    },
    onError: failed,
  });

  const disable = useMutation({
    mutationFn: (proof: mfaApi.Proof) => mfaApi.disableTotp(proof),
    onSuccess: () => {
      setDisabling(false);
      toast.success(t('security.mfa.disabled'), {
        description: t('security.mfa.others_signed_out'),
      });
      void refresh();
    },
    onError: failed,
  });

  const regenerate = useMutation({
    mutationFn: (proof: mfaApi.Proof) => mfaApi.regenerateRecoveryCodes(proof),
    onSuccess: (next) => {
      setRegenerating(false);
      setCodes(next);
      void refresh();
    },
    onError: failed,
  });

  const setupDone = (recoveryCodes: string[] | null) => {
    setSetup(null);
    void refresh();

    toast.success(t(replacing ? 'security.mfa.replaced' : 'security.mfa.enabled'), {
      ...(replacing || recoveryCodes
        ? { description: t('security.mfa.others_signed_out') }
        : {}),
    });

    if (recoveryCodes) setCodes(recoveryCodes);
  };

  const status = statusQuery.data;
  const totpOn = status?.totpEnabled ?? false;
  const mfaOn = totpOn || (status?.passkeys.length ?? 0) > 0;
  const methods = factorMethods(status);
  const asksPassword = status?.addingRequires === 'password';

  const beginSetup = (mode: 'enable' | 'replace') => {
    setReplacing(mode === 'replace');

    if (status?.addingRequires === 'nothing') start.mutate(undefined);
    else setConfirming(mode);
  };

  const lastFactorKept =
    status && status.requiredBy.length > 0 && status.passkeys.length === 0
      ? t('security.mfa.required_by', { organizations: status.requiredBy.join(', ') })
      : undefined;

  const signInAgain =
    status?.addingRequires === 'sign_in_again' ? t('security.mfa.sign_in_again') : undefined;

  const action = statusQuery.isError ? (
    <span className="font-sans text-xs text-red-soft">{t('security.mfa.load_failed')}</span>
  ) : !status ? null : totpOn ? (
    <div className="flex flex-wrap items-center gap-2">
      <span className="mr-1 font-sans text-xs font-strong text-[var(--green-ink)]">
        {t('security.mfa.on')}
      </span>
      <ChromeButton onClick={() => beginSetup('replace')}>{t('security.mfa.replace')}</ChromeButton>
      <ChromeButton
        disabled={lastFactorKept !== undefined}
        disabledReason={lastFactorKept}
        onClick={() => setDisabling(true)}
      >
        {t('security.mfa.disable')}
      </ChromeButton>
    </div>
  ) : (
    <ChromeButton
      variant="primary"
      disabled={signInAgain !== undefined || start.isPending}
      disabledReason={signInAgain}
      onClick={() => beginSetup('enable')}
    >
      {t('security.mfa.enable')}
    </ChromeButton>
  );

  return (
    <>
      <SecurityRow
        testId="mfa-row"
        label={t('security.mfa.title')}
        description={
          totpOn && status?.totpEnabledAt
            ? t('security.mfa.description_on', {
                time: time.micros(status.totpEnabledAt, false),
              })
            : t('security.mfa.description_off')
        }
        action={action}
      />

      {status && (
        <PasskeySection
          status={status}
          methods={methods}
          onChanged={() => void refresh()}
          onRecoveryCodes={setCodes}
        />
      )}

      {mfaOn && (
        <SecurityRow
          testId="recovery-codes-row"
          label={t('security.mfa.recovery.title')}
          description={t('security.mfa.recovery.description', {
            count: status?.recoveryCodesLeft ?? 0,
          })}
          action={
            <ChromeButton onClick={() => setRegenerating(true)}>
              {t('security.mfa.recovery.regenerate')}
            </ChromeButton>
          }
        />
      )}

      <ProofDialog
        testId="mfa-confirm-dialog"
        open={confirming !== null}
        onOpenChange={(open) => !open && setConfirming(null)}
        methods={methods}
        password={asksPassword}
        title={t('security.mfa.confirm_title')}
        description={
          asksPassword ? t('security.mfa.confirm_password') : t('security.mfa.confirm_factor')
        }
        confirmLabel={t('security.mfa.continue')}
        busyLabel={t('security.mfa.checking')}
        busy={start.isPending}
        failures={failures}
        onConfirm={(proof) => start.mutate(proof)}
      />

      <SetupDialog
        setup={setup}
        replacing={replacing}
        onClose={() => setSetup(null)}
        onDone={setupDone}
      />

      <ProofDialog
        testId="mfa-disable-dialog"
        open={disabling}
        onOpenChange={setDisabling}
        methods={methods}
        destructive
        title={t('security.mfa.disable_title')}
        description={
          (status?.passkeys.length ?? 0) > 0
            ? t('security.mfa.disable_description_keep')
            : t('security.mfa.disable_description')
        }
        confirmLabel={t('security.mfa.disable')}
        busyLabel={t('security.mfa.disabling')}
        busy={disable.isPending}
        failures={failures}
        onConfirm={(proof) => disable.mutate(proof)}
      />

      <ProofDialog
        testId="recovery-regenerate-dialog"
        open={regenerating}
        onOpenChange={setRegenerating}
        methods={methods}
        title={t('security.mfa.recovery.regenerate_title')}
        description={t('security.mfa.recovery.regenerate_description')}
        confirmLabel={t('security.mfa.recovery.regenerate')}
        busyLabel={t('security.mfa.recovery.regenerating')}
        busy={regenerate.isPending}
        failures={failures}
        onConfirm={(proof) => regenerate.mutate(proof)}
      />

      <RecoveryCodesDialog codes={codes} email={email} onClose={() => setCodes(null)} />
    </>
  );
}
