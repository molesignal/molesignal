import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';

export function OneTimeApiTokenDialog({
  token,
  onClose,
}: {
  token: string | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('agent');

  const copy = async () => {
    if (!token) return;
    try {
      await navigator.clipboard.writeText(token);
      toast.success(t('approvals.one_time_token_copied'));
    } catch {
      toast.error(t('approvals.one_time_token_copy_failed'));
    }
  };

  return (
    <Dialog open={Boolean(token)} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="w-[min(560px,calc(100vw-24px))]">
        <DialogHeader>
          <DialogTitle>{t('approvals.one_time_token_title')}</DialogTitle>
          <DialogDescription>
            {t('approvals.one_time_token_description')}
          </DialogDescription>
        </DialogHeader>
        <div className="flex min-w-0 items-center gap-2 rounded-md border border-bd-0 bg-bg-2 p-2">
          <code className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-xs text-tx-0">
            {token}
          </code>
          <CopyIconButton
            type="button"
            onClick={copy}
            label={t('approvals.copy_one_time_token')}
          />
        </div>
        <DialogFooter>
          <ChromeButton variant="primary" onClick={onClose}>
            {t('common.close')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
