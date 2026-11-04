import { useMutation } from '@tanstack/react-query';
import { KeyRound } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { MfaMethod } from '@/api/auth';
import * as mfaApi from '@/api/mfa';
import { useTimeFormatter } from '@/lib/time';
import {
  createPasskey,
  type PasskeyCeremony,
  type PasskeyCreationOptions,
  passkeysSupported,
} from '@/lib/webauthn';
import { ChromeButton } from '@/shell/chrome';
import { FormField } from '@/shell/FormDrawer';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { Input } from '@/shell/ui/input';
import { toast } from '@/shell/ui/sonner';

import { useCodeErrorToast } from './CodeField';
import { passkeyProvider } from './passkeyProviders';
import { ProofDialog } from './ProofDialog';

/** The name a passkey goes by: its own, else where it is kept. */
function usePasskeyLabel() {
  const { t } = useTranslation('account');

  return (passkey: mfaApi.Passkey) =>
    passkey.name ||
    passkeyProvider(passkey.aaguid, (key) => t(key as never)) ||
    t('security.passkeys.unnamed');
}

/**
 * The name of a new passkey, and the button that asks the browser to create it:
 * the options are already here, because only a click may start the browser's prompt
 * on Safari. Backing out of the prompt leaves the dialog open, ready to try again.
 */
function AddDialog({
  prepared,
  onClose,
  onAdded,
}: {
  /** `null` while closed. */
  prepared: PasskeyCeremony<PasskeyCreationOptions> | null;
  onClose: () => void;
  onAdded: (recoveryCodes: string[] | null) => void;
}) {
  const { t } = useTranslation(['account', 'common']);
  const failed = useCodeErrorToast();
  const [name, setName] = React.useState('');
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => setName(''), [prepared]);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (busy || !prepared) return;
    setBusy(true);

    try {
      const credential = await createPasskey(prepared.options);

      const added = await mfaApi.addPasskey({
        ceremony: prepared.ceremony,
        credential,
        name,
      });

      onAdded(added.recoveryCodes);
    } catch (error) {
      failed(error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={prepared !== null} onOpenChange={(open) => !open && !busy && onClose()}>
      <DialogContent data-testid="passkey-add-dialog">
        <form className="flex min-w-0 flex-col gap-4" onSubmit={(event) => void submit(event)}>
          <DialogHeader>
            <DialogTitle>{t('security.passkeys.add')}</DialogTitle>
            <DialogDescription>{t('security.passkeys.add_description')}</DialogDescription>
          </DialogHeader>
          <FormField label={t('security.passkeys.name')}>
            <Input
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t('security.passkeys.name_placeholder')}
              maxLength={64}
              disabled={busy}
              autoFocus
              data-testid="passkey-name-input"
            />
          </FormField>
          <DialogFooter>
            <ChromeButton type="button" disabled={busy} onClick={onClose}>
              {t('common:actions.cancel')}
            </ChromeButton>
            <ChromeButton type="submit" variant="primary" disabled={busy}>
              {busy ? t('security.passkeys.adding') : t('security.passkeys.create')}
            </ChromeButton>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function RenameDialog({
  passkey,
  onClose,
  onRenamed,
}: {
  /** `null` while closed. */
  passkey: mfaApi.Passkey | null;
  onClose: () => void;
  onRenamed: () => void;
}) {
  const { t } = useTranslation(['account', 'common']);
  const failed = useCodeErrorToast();
  const [name, setName] = React.useState('');

  React.useEffect(() => setName(passkey?.name ?? ''), [passkey]);

  const rename = useMutation({
    mutationFn: () => mfaApi.renamePasskey(passkey?.id ?? '', name),
    onSuccess: onRenamed,
    onError: failed,
  });

  return (
    <Dialog
      open={passkey !== null}
      onOpenChange={(open) => !open && !rename.isPending && onClose()}
    >
      <DialogContent data-testid="passkey-rename-dialog">
        <form
          className="flex min-w-0 flex-col gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (!rename.isPending) rename.mutate();
          }}
        >
          <DialogHeader>
            <DialogTitle>{t('security.passkeys.rename_title')}</DialogTitle>
            <DialogDescription>{t('security.passkeys.rename_description')}</DialogDescription>
          </DialogHeader>
          <FormField label={t('security.passkeys.name')}>
            <Input
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t('security.passkeys.name_placeholder')}
              maxLength={64}
              disabled={rename.isPending}
              autoFocus
            />
          </FormField>
          <DialogFooter>
            <ChromeButton type="button" disabled={rename.isPending} onClick={onClose}>
              {t('common:actions.cancel')}
            </ChromeButton>
            <ChromeButton type="submit" variant="primary" disabled={rename.isPending}>
              {t('common:actions.save')}
            </ChromeButton>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/**
 * The account's passkeys on the security page: adding one (after confirming it's the
 * owner), naming and removing them. Unavailable passkeys keep the button, greyed out
 * with the reason: the instance's external URL, or the browser.
 */
export function PasskeySection({
  status,
  methods,
  onChanged,
  onRecoveryCodes,
}: {
  status: mfaApi.MfaStatus;
  /** What the account can confirm a change with. */
  methods: MfaMethod[];
  onChanged: () => void;
  onRecoveryCodes: (codes: string[]) => void;
}) {
  const { t } = useTranslation(['account', 'common']);
  const time = useTimeFormatter();
  const label = usePasskeyLabel();
  const failed = useCodeErrorToast();
  const [confirming, setConfirming] = React.useState(false);
  const [prepared, setPrepared] = React.useState<PasskeyCeremony<PasskeyCreationOptions> | null>(
    null,
  );
  const [renaming, setRenaming] = React.useState<mfaApi.Passkey | null>(null);
  const [removing, setRemoving] = React.useState<mfaApi.Passkey | null>(null);
  const [failures, setFailures] = React.useState(0);

  const fail = (error: unknown) => {
    setFailures((count) => count + 1);
    failed(error);
  };

  const start = useMutation({
    mutationFn: (proof?: mfaApi.Proof) => mfaApi.passkeyOptions(proof),
    onSuccess: (next) => {
      setConfirming(false);
      setPrepared(next);
    },
    onError: fail,
  });

  const remove = useMutation({
    mutationFn: ({ id, proof }: { id: string; proof: mfaApi.Proof }) =>
      mfaApi.removePasskey(id, proof),
    onSuccess: () => {
      setRemoving(null);
      toast.success(t('security.passkeys.removed'), {
        description: t('security.mfa.others_signed_out'),
      });
      onChanged();
    },
    onError: fail,
  });

  const added = (recoveryCodes: string[] | null) => {
    setPrepared(null);
    onChanged();
    toast.success(t('security.passkeys.added'), {
      ...(recoveryCodes ? { description: t('security.mfa.others_signed_out') } : {}),
    });
    if (recoveryCodes) onRecoveryCodes(recoveryCodes);
  };

  const unavailable = !passkeysSupported()
    ? t('security.passkeys.unavailable.browser')
    : status.passkeysUnavailable
      ? t(`security.passkeys.unavailable.${status.passkeysUnavailable}`)
      : status.addingRequires === 'sign_in_again'
        ? t('security.mfa.sign_in_again')
        : undefined;

  const asksPassword = status.addingRequires === 'password';
  const last = !status.totpEnabled && status.passkeys.length === 1;
  const lastFactorKept =
    last && status.requiredBy.length > 0
      ? t('security.mfa.required_by', { organizations: status.requiredBy.join(', ') })
      : undefined;

  return (
    <div className="py-4" data-testid="passkeys">
      <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
        <div>
          <div className="font-sans text-sm font-strong text-tx-0">
            {t('security.passkeys.title')}
          </div>
          <div className="mt-1 max-w-xl font-sans text-xs leading-relaxed text-tx-2">
            {t('security.passkeys.description')}
          </div>
        </div>
        <div>
          <ChromeButton
            disabled={unavailable !== undefined || start.isPending}
            disabledReason={unavailable}
            onClick={() => {
              if (status.addingRequires === 'nothing') start.mutate(undefined);
              else setConfirming(true);
            }}
          >
            {t('security.passkeys.add')}
          </ChromeButton>
        </div>
      </div>

      {status.passkeys.length > 0 && (
        <ul className="mt-3 flex flex-col gap-2">
          {status.passkeys.map((passkey) => {
            const provider = passkeyProvider(passkey.aaguid, (key) => t(key as never));

            return (
              <li key={passkey.id} className="flex items-center gap-3">
                <div className="grid h-9 w-9 shrink-0 place-items-center rounded-md bg-bg-2 text-tx-2">
                  <KeyRound size={14} aria-hidden />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate font-sans text-sm text-tx-0">{label(passkey)}</div>
                  <div className="mt-0.5 truncate font-sans text-xs text-tx-2">
                    {[
                      passkey.name ? provider : null,
                      t('security.passkeys.added_at', {
                        time: time.micros(passkey.createdAt, false),
                      }),
                      passkey.lastUsedAt
                        ? t('security.passkeys.last_used', {
                            time: time.micros(passkey.lastUsedAt, false),
                          })
                        : t('security.passkeys.never_used'),
                    ]
                      .filter(Boolean)
                      .join(' · ')}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <ChromeButton onClick={() => setRenaming(passkey)}>
                    {t('security.passkeys.rename')}
                  </ChromeButton>
                  <ChromeButton
                    disabled={lastFactorKept !== undefined}
                    disabledReason={lastFactorKept}
                    onClick={() => setRemoving(passkey)}
                  >
                    {t('common:actions.delete')}
                  </ChromeButton>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      <ProofDialog
        testId="passkey-confirm-dialog"
        open={confirming}
        onOpenChange={setConfirming}
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

      <AddDialog prepared={prepared} onClose={() => setPrepared(null)} onAdded={added} />

      <RenameDialog
        passkey={renaming}
        onClose={() => setRenaming(null)}
        onRenamed={() => {
          setRenaming(null);
          onChanged();
        }}
      />

      <ProofDialog
        testId="passkey-remove-dialog"
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        methods={methods}
        destructive
        title={t('security.passkeys.remove_title', { name: removing ? label(removing) : '' })}
        description={
          last
            ? t('security.passkeys.remove_description_last')
            : t('security.passkeys.remove_description')
        }
        confirmLabel={t('common:actions.delete')}
        busyLabel={t('security.passkeys.removing')}
        busy={remove.isPending}
        failures={failures}
        onConfirm={(proof) => removing && remove.mutate({ id: removing.id, proof })}
      />
    </div>
  );
}
