import { Copy, Download } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { writeClipboardText } from '@/lib/clipboard';
import { ChromeButton } from '@/shell/chrome';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';

/** The file the codes are saved as. */
const FILE_NAME = 'molesignal-recovery-codes.txt';

/** A text file of `text` handed to the browser to save. */
function saveTextFile(text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = FILE_NAME;
  document.body.append(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

/**
 * A new set of recovery codes, shown this once: the server keeps only their
 * digests. The dialog closes only through "I've saved them".
 */
export function RecoveryCodesDialog({
  codes,
  email,
  onClose,
}: {
  codes: string[] | null;
  email: string;
  onClose: () => void;
}) {
  const { t } = useTranslation('account');
  const text = codes
    ? `${[
        t('security.mfa.recovery.file_heading', { email }),
        t('security.mfa.recovery.file_note'),
        '',
        ...codes,
      ].join('\n')}\n`
    : '';

  const copy = async () => {
    if (!codes) return;
    try {
      await writeClipboardText(codes.join('\n'));
      toast.success(t('security.mfa.recovery.copied'));
    } catch {
      toast.error(t('security.mfa.recovery.copy_failed'));
    }
  };

  return (
    <Dialog open={codes !== null}>
      <DialogContent
        hideClose
        onEscapeKeyDown={(event) => event.preventDefault()}
        onPointerDownOutside={(event) => event.preventDefault()}
        data-testid="recovery-codes-dialog"
      >
        <DialogHeader>
          <DialogTitle>{t('security.mfa.recovery.save_title')}</DialogTitle>
          <DialogDescription>{t('security.mfa.recovery.save_description')}</DialogDescription>
        </DialogHeader>
        <ol
          className="grid min-w-0 grid-cols-2 gap-x-6 gap-y-2 rounded-md bg-[var(--control-surface)] px-4 py-3"
          aria-label={t('security.mfa.recovery.title')}
        >
          {(codes ?? []).map((code) => (
            <li key={code} className="font-mono text-sm tabular-nums text-tx-0">
              {code}
            </li>
          ))}
        </ol>
        <DialogFooter className="sm:justify-between">
          <div className="flex gap-2">
            <ChromeButton type="button" onClick={() => void copy()}>
              <Copy className="h-3.5 w-3.5" />
              {t('security.mfa.recovery.copy')}
            </ChromeButton>
            <ChromeButton type="button" onClick={() => saveTextFile(text)}>
              <Download className="h-3.5 w-3.5" />
              {t('security.mfa.recovery.download')}
            </ChromeButton>
          </div>
          <ChromeButton type="button" variant="primary" onClick={onClose}>
            {t('security.mfa.recovery.saved')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
