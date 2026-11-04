import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as mfaApi from '@/api/mfa';
import { CopyableValue } from '@/routes/settings/_atoms';
import { ChromeButton } from '@/shell/chrome';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';

import { CodeField, codeComplete, useCodeErrorToast } from './CodeField';

/**
 * Sets up an authenticator: scan the QR code (or type the key), then enter the
 * first code it shows. `onDone` gets the recovery codes when this turned
 * multi-factor authentication on.
 */
export function SetupDialog({
  setup,
  replacing,
  onClose,
  onDone,
}: {
  /** `null` while closed. */
  setup: mfaApi.TotpSetup | null;
  replacing: boolean;
  onClose: () => void;
  onDone: (recoveryCodes: string[] | null) => void;
}) {
  const { t } = useTranslation(['account', 'common']);
  const codeError = useCodeErrorToast();
  const [code, setCode] = React.useState('');

  const confirm = useMutation({
    mutationFn: (first: string) => {
      if (!setup) throw new Error('authenticator setup is not ready');
      return mfaApi.confirmTotp(setup.setup, first);
    },
    onSuccess: (result) => onDone(result.recoveryCodes),
    onError: (error) => {
      setCode('');
      codeError(error);
    },
  });

  React.useEffect(() => setCode(''), [setup]);

  const busy = confirm.isPending;

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!busy && codeComplete('totp', code)) confirm.mutate(code);
  };

  return (
    <Dialog open={setup !== null} onOpenChange={(open) => !open && !busy && onClose()}>
      <DialogContent data-testid="mfa-setup-dialog">
        <form className="flex min-w-0 flex-col gap-4" onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>
              {replacing ? t('security.mfa.replace_title') : t('security.mfa.setup_title')}
            </DialogTitle>
            <DialogDescription>{t('security.mfa.setup_description')}</DialogDescription>
          </DialogHeader>

          {setup && (
            <div className="flex min-w-0 flex-col gap-4">
              {/* White under the code in either theme: scanners read dark on light. */}
              <img
                src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(setup.qrSvg)}`}
                alt={t('security.mfa.qr_alt')}
                className="h-44 w-44 rounded-md bg-white"
                data-testid="mfa-qr"
              />
              <div className="flex min-w-0 flex-col gap-1.5">
                <span className="font-sans text-xs font-strong text-tx-1">
                  {t('security.mfa.secret')}
                </span>
                <CopyableValue
                  value={setup.secret}
                  copyLabel={t('security.mfa.copy_secret')}
                  copiedLabel={t('security.mfa.secret_copied')}
                />
              </div>
            </div>
          )}

          <CodeField
            methods={['totp']}
            method="totp"
            onMethodChange={() => undefined}
            code={code}
            onCodeChange={setCode}
            disabled={busy}
          />

          <DialogFooter>
            <ChromeButton type="button" disabled={busy} onClick={onClose}>
              {t('common:actions.cancel')}
            </ChromeButton>
            <ChromeButton
              type="submit"
              variant="primary"
              disabled={busy || !codeComplete('totp', code)}
            >
              {busy
                ? t('security.mfa.enabling')
                : replacing
                  ? t('security.mfa.confirm_replace')
                  : t('security.mfa.confirm_enable')}
            </ChromeButton>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
