import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { MfaMethod } from '@/api/auth';
import * as mfaApi from '@/api/mfa';
import { usePreparedCeremony } from '@/lib/usePreparedCeremony';
import { getPasskey } from '@/lib/webauthn';
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

import { CodeField, codeComplete, useCodeErrorToast } from './CodeField';

/**
 * Asks for a confirmation before a change to multi-factor authentication: one of
 * the account's second factors (`methods`, the first one offered first), or the
 * password while it has none (`password`). What was typed empties after each
 * failed try (`failures` goes up), which also gets a new passkey ceremony: the
 * server has used the last one up.
 */
export function ProofDialog({
  open,
  onOpenChange,
  methods,
  password = false,
  title,
  description,
  confirmLabel,
  busyLabel,
  destructive = false,
  busy,
  failures,
  onConfirm,
  testId,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  methods: MfaMethod[];
  password?: boolean;
  title: string;
  description: string;
  confirmLabel: string;
  busyLabel: string;
  destructive?: boolean;
  busy: boolean;
  failures: number;
  onConfirm: (proof: mfaApi.Proof) => void;
  testId?: string;
}) {
  const { t } = useTranslation(['account', 'common']);
  const failed = useCodeErrorToast();
  const first = methods[0] ?? 'totp';
  const [method, setMethod] = React.useState<MfaMethod>(first);
  const [value, setValue] = React.useState('');
  const [asking, setAsking] = React.useState(false);
  const passkey = !password && method === 'passkey';
  const prepared = usePreparedCeremony(mfaApi.passkeyProof, open && passkey, failures, failed);
  const working = busy || asking;

  const ready = password
    ? value.length > 0
    : passkey
      ? prepared !== null
      : codeComplete(method, value);

  React.useEffect(() => {
    if (!open) return;
    setMethod(first);
    setValue('');
  }, [open, first]);

  React.useEffect(() => setValue(''), [failures]);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (working || !ready) return;

    if (password) {
      onConfirm({ method: 'password', password: value });
      return;
    }

    if (method !== 'passkey') {
      onConfirm({ method, code: value });
      return;
    }

    if (!prepared) return;
    setAsking(true);

    try {
      const credential = await getPasskey(prepared.options);
      onConfirm({ method: 'passkey', ceremony: prepared.ceremony, credential });
    } catch (error) {
      failed(error);
    } finally {
      setAsking(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !working && onOpenChange(next)}>
      <DialogContent data-testid={testId}>
        <form className="flex min-w-0 flex-col gap-4" onSubmit={(event) => void submit(event)}>
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            <DialogDescription>{description}</DialogDescription>
          </DialogHeader>
          {password ? (
            <FormField label={t('account:security.mfa.password')}>
              <Input
                type="password"
                value={value}
                onChange={(event) => setValue(event.target.value)}
                autoComplete="current-password"
                disabled={working}
                autoFocus
                data-testid="mfa-password-input"
              />
            </FormField>
          ) : (
            <CodeField
              methods={methods}
              method={method}
              onMethodChange={setMethod}
              code={value}
              onCodeChange={setValue}
              disabled={working}
            />
          )}
          <DialogFooter>
            <ChromeButton type="button" disabled={working} onClick={() => onOpenChange(false)}>
              {t('common:actions.cancel')}
            </ChromeButton>
            <ChromeButton
              type="submit"
              variant={destructive ? 'danger' : 'primary'}
              disabled={working || !ready}
            >
              {working ? busyLabel : confirmLabel}
            </ChromeButton>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
