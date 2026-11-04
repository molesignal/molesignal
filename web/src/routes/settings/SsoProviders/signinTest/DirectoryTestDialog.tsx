import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';

/**
 * A directory has no sign-in page to go to: it is tested with an account typed here.
 * The password stays in this dialog until it is sent, and goes when it closes.
 */
export function DirectoryTestDialog({
  providerName,
  open,
  busy,
  onClose,
  onSubmit,
}: {
  providerName: string;
  open: boolean;
  busy: boolean;
  onClose: () => void;
  onSubmit: (username: string, password: string) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const [username, setUsername] = React.useState('');
  const [password, setPassword] = React.useState('');

  React.useEffect(() => {
    if (!open) {
      setUsername('');
      setPassword('');
    }
  }, [open]);

  const ready = Boolean(username.trim() && password);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (ready && !busy) onSubmit(username.trim(), password);
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="w-[420px]" data-testid="sso-directory-test">
        <DialogHeader>
          <DialogTitle>{t('sso_providers.test.directory.title', { name: providerName })}</DialogTitle>
          <DialogDescription>{t('sso_providers.test.directory.description')}</DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="mt-3 flex min-w-0 flex-col gap-3">
          <FormField label={t('sso_providers.test.directory.username')} required>
            <FormInput
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              autoComplete="off"
              spellCheck={false}
              autoFocus
            />
          </FormField>
          <FormField label={t('sso_providers.test.directory.password')} required>
            <FormInput
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              autoComplete="new-password"
            />
          </FormField>
          <div className="mt-1 flex items-center justify-end gap-2">
            <ChromeButton disabled={busy} onClick={onClose}>
              {tc('actions.cancel')}
            </ChromeButton>
            <ChromeButton
              type="submit"
              variant="primary"
              disabled={busy || !ready}
              disabledReason={
                busy ? tc('access.operation_pending') : !ready ? tc('access.form_invalid') : undefined
              }
            >
              {busy ? tc('status.working') : t('sso_providers.test.directory.submit')}
            </ChromeButton>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
