import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';

export interface ConfirmConsequence {
  label: React.ReactNode;
  value: React.ReactNode;
}

interface ConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: React.ReactNode;
  description?: React.ReactNode;
  /** What goes with it, one fact to a row, said before it goes. */
  consequences?: readonly ConfirmConsequence[] | undefined;
  /** The name to type before the action opens. For what cannot be taken back. */
  typeToConfirm?: string | undefined;
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
 * styling stays consistent with the rest of the app. The harder a thing is to
 * undo the more it asks: a click, the facts of what goes with it, the name typed.
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  consequences,
  typeToConfirm,
  confirmLabel,
  cancelLabel,
  destructive = false,
  busy = false,
  disabled = false,
  disabledReason,
  busyLabel,
  onConfirm,
}: ConfirmDialogProps) {
  const { t } = useTranslation('common');
  const [typed, setTyped] = React.useState('');

  // Every opening starts over: a name typed for one row does not carry to the next.
  React.useEffect(() => {
    if (open) setTyped('');
  }, [open]);

  const pendingReason = busy ? t('access.operation_pending') : undefined;
  const awaitingName = Boolean(typeToConfirm) && typed !== typeToConfirm;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[420px]">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {description && <DialogDescription>{description}</DialogDescription>}
        </DialogHeader>
        {consequences && consequences.length > 0 && (
          <dl data-confirm-consequences className="mt-3 border-t border-bd-0 font-sans text-sm">
            {consequences.map((fact, index) => (
              <div key={index} className="flex items-baseline gap-3 border-b border-bd-0 py-2">
                <dt className="w-20 shrink-0 text-tx-2">{fact.label}</dt>
                <dd className="min-w-0 flex-1 text-tx-0">{fact.value}</dd>
              </div>
            ))}
          </dl>
        )}
        {typeToConfirm && (
          <label className="mt-3 flex flex-col gap-1.5 font-sans text-sm text-tx-1">
            <span>{t('confirm_dialog.type_to_confirm', { name: typeToConfirm })}</span>
            <FormInput
              value={typed}
              onChange={(event) => setTyped(event.target.value)}
              autoComplete="off"
              spellCheck={false}
              data-testid="confirm-type-name"
            />
          </label>
        )}
        <div className="mt-3 flex items-center justify-end gap-2">
          <ChromeButton
            disabled={busy}
            disabledReason={pendingReason}
            onClick={() => onOpenChange(false)}
          >
            {cancelLabel ?? t('actions.cancel')}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={busy || disabled || awaitingName}
            disabledReason={
              disabledReason ?? pendingReason ?? (awaitingName ? t('confirm_dialog.type_required') : undefined)
            }
            onClick={() => {
              if (disabled || awaitingName) return;
              void onConfirm();
            }}
            className={
              destructive
                ? 'border-red bg-red text-[var(--red-fg)] enabled:hover:bg-red-soft'
                : undefined
            }
          >
            {busy ? (busyLabel ?? t('status.working')) : (confirmLabel ?? t('actions.confirm'))}
          </ChromeButton>
        </div>
      </DialogContent>
    </Dialog>
  );
}
