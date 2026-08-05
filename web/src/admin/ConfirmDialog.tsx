import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';

interface ConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: React.ReactNode;
  description?: React.ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  destructive?: boolean;
  busy?: boolean;
  disabled?: boolean;
  disabledReason?: React.ReactNode;
  busyLabel?: string;
  onConfirm: () => void | Promise<void> | unknown;
}

/**
 * Modal "are you sure?" used by every admin destructive action (delete
 * function, revoke invite, drop policy). Wraps the shadcn Dialog so the
 * styling stays consistent with the rest of the app.
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel = 'Confirm',
  cancelLabel = 'Cancel',
  destructive = false,
  busy = false,
  disabled = false,
  disabledReason,
  busyLabel = 'Working…',
  onConfirm,
}: ConfirmDialogProps) {
  const { t } = useTranslation('common');
  const pendingReason = busy ? t('access.operation_pending') : undefined;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[420px]">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {description && <DialogDescription>{description}</DialogDescription>}
        </DialogHeader>
        <div className="mt-3 flex items-center justify-end gap-2">
          <ChromeButton
            disabled={busy}
            disabledReason={pendingReason}
            onClick={() => onOpenChange(false)}
          >
            {cancelLabel}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={busy || disabled}
            disabledReason={disabledReason ?? pendingReason}
            onClick={() => {
              if (disabled) return;
              void onConfirm();
            }}
            className={
              destructive
                ? 'border-red bg-red text-tx-0 enabled:hover:bg-red-soft'
                : undefined
            }
          >
            {busy ? busyLabel : confirmLabel}
          </ChromeButton>
        </div>
      </DialogContent>
    </Dialog>
  );
}
