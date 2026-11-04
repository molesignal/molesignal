import { useTranslation } from 'react-i18next';

import type { IssuedAlertSource } from '@/api/alertSources';
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

import { intakeAddress, setupSnippet } from './model';
import { Labeled, SnippetBlock } from './Section';

/**
 * A source's token, the one time it is seen: right after the source was made
 * or its token was rotated. What the page shows with `<TOKEN>` in it (the
 * sender's setup, or the address of a sender that carries the token there) is
 * written out with the token, so it can be pasted as it is.
 */
export function SourceTokenDialog({
  issued,
  origin,
  onClose,
}: {
  issued: IssuedAlertSource | null;
  /** Where the sender reaches MoleSignal. */
  origin: string;
  onClose: () => void;
}) {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');

  if (!issued) return null;

  const address = intakeAddress(origin, issued, issued.token);
  const setup = setupSnippet(issued.kind, address, issued.token);

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      {/* The dialog's own cap is narrower than a setup with a token in it. On the page's own
          surface, the values sit in the same wells as on the page; the dialog's gray hides them. */}
      <DialogContent className="w-[min(680px,calc(100vw-24px))] max-w-none bg-[var(--functional-surface)]">
        <DialogHeader>
          <DialogTitle>{t('alert_sources.token.title', { name: issued.name })}</DialogTitle>
          <DialogDescription>{t('alert_sources.token.shown_once')}</DialogDescription>
        </DialogHeader>
        {/* Long lines wrap inside the dialog instead of widening it. */}
        <div data-source-token className="flex min-w-0 flex-col gap-4">
          <Labeled label={t('alert_sources.detail.token')}>
            <CopyableValue
              value={issued.token}
              copyLabel={t('alert_sources.token.copy')}
              copiedLabel={tc('actions.copied')}
            />
          </Labeled>
          {setup ? (
            <Labeled label={t('alert_sources.detail.setup')}>
              <SnippetBlock content={setup.content} label={t('alert_sources.detail.copy_setup')} />
            </Labeled>
          ) : (
            <Labeled label={t('alert_sources.detail.address')}>
              <SnippetBlock content={address} label={t('alert_sources.detail.copy_address')} />
            </Labeled>
          )}
        </div>
        <DialogFooter>
          <ChromeButton variant="primary" onClick={onClose}>
            {t('alert_sources.token.close')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
