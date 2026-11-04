import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';

/**
 * Asked before work that was not saved goes away: closing a drawer ('discard')
 * or leaving a page ('leave'). Staying is the first thing offered and is what
 * Escape and a click outside mean.
 */
export function UnsavedPrompt({
  open,
  kind,
  onKeep,
  onDiscard,
}: {
  open: boolean;
  kind: 'discard' | 'leave';
  onKeep: () => void;
  onDiscard: () => void;
}) {
  const { t } = useTranslation('common');

  return (
    <Dialog open={open} onOpenChange={(next) => !next && onKeep()}>
      <DialogContent className="w-[420px]" data-testid="unsaved-prompt">
        <DialogHeader>
          <DialogTitle>{t(kind === 'leave' ? 'unsaved.leave_title' : 'unsaved.discard_title')}</DialogTitle>
          <DialogDescription>
            {t(kind === 'leave' ? 'unsaved.leave_description' : 'unsaved.discard_description')}
          </DialogDescription>
        </DialogHeader>
        <div className="mt-3 flex items-center justify-end gap-2">
          <ChromeButton onClick={onKeep}>{t('unsaved.keep_editing')}</ChromeButton>
          <ChromeButton
            variant="primary"
            onClick={onDiscard}
            className="border-red bg-red text-[var(--red-fg)] enabled:hover:bg-red-soft"
          >
            {t(kind === 'leave' ? 'unsaved.leave' : 'unsaved.discard')}
          </ChromeButton>
        </div>
      </DialogContent>
    </Dialog>
  );
}
