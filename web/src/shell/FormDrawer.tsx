import * as Dialog from '@radix-ui/react-dialog';
import { ChevronDown, ChevronUp, Plus, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton, uiLabelClass, uiLabelStrongClass } from '@/shell/chrome';
import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';
import { RadioGroup, RadioGroupItem } from '@/shell/ui/radio-group';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

/**
 * 720px right-side form drawer with three regions: header / body / footer.
 * Shared by every "new / edit X" flow in the app.
 */
export function FormDrawer({
  open,
  onOpenChange,
  title,
  subtitle,
  width = 760,
  bodyClassName,
  children,
  footer,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  width?: number | string;
  bodyClassName?: string;
  children: React.ReactNode;
  footer?: React.ReactNode;
}) {
  const { t } = useTranslation('common');

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-overlay data-[state=open]:animate-fade-in" />
        <Dialog.Content
          className="fixed inset-y-0 right-0 z-50 flex flex-col border-l border-bd-1 bg-bg-1 shadow-drawer data-[state=open]:animate-slide-in-right"
          style={{ width, maxWidth: 'calc(100vw - 16px)' }}
        >
          {/* header */}
          <div className="flex items-start gap-4 border-b border-bd-0 px-6 py-5">
            <div className="flex-1">
              <Dialog.Title className="m-0 font-sans text-xl font-display-strong tracking-[-0.02em] text-tx-0">
                {title}
              </Dialog.Title>
              {subtitle && (
                <Dialog.Description className="mt-1.5 text-sm text-tx-2">{subtitle}</Dialog.Description>
              )}
            </div>
            <Dialog.Close asChild>
              <button
                type="button"
                aria-label={t('actions.close')}
                className="flex h-8 w-8 items-center justify-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0"
              >
                <X className="h-4 w-4" />
              </button>
            </Dialog.Close>
          </div>

          {/* body */}
          <div className={cn('flex-1 overflow-auto px-6 py-5', bodyClassName)}>{children}</div>

          {/* footer */}
          {footer && (
            <div className="flex items-center justify-end gap-2 border-t border-bd-0 bg-bg-2 px-6 py-4">
              {footer}
            </div>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/* ───────────────────────── Form atoms ───────────────────────── */

export function FormSection({
  title,
  description,
  children,
  className,
}: {
  title?: string;
  description?: string;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={cn('mb-8', className)}>
      {title && (
        <div className={cn('mb-1.5', uiLabelStrongClass)}>
          {title}
        </div>
      )}
      {description && <div className="mb-4 text-xs leading-relaxed text-tx-3">{description}</div>}
      <div className="flex flex-col gap-4">{children}</div>
    </section>
  );
}

export function FormField({
  label,
  hint,
  required,
  children,
  className,
}: {
  label: string;
  hint?: string;
  required?: boolean;
  children: React.ReactNode;
  className?: string;
}) {
  // `<label>` wraps children so the child input is implicitly labeled. axe
  // critical rule `label` (every form element has a label) passes without
  // forcing each call site to wire `htmlFor`/`id` pairs.
  return (
    <label className={cn('flex cursor-default flex-col gap-1.5', className)}>
      <span className={cn('flex items-center gap-1', uiLabelClass)}>
        {label}
        {required && (
          // Required asterisk uses the status `red` token — the asterisk
          // is a hint that this field is incomplete, not a brand surface.
          <span className="text-red" aria-hidden>
            *
          </span>
        )}
      </span>
      {children}
      {hint && <span className="text-xs leading-relaxed text-tx-3">{hint}</span>}
    </label>
  );
}

export function FormRow({ children, className }: { children: React.ReactNode; className?: string }) {
  return <div className={cn('grid grid-cols-2 gap-3', className)}>{children}</div>;
}

export const FormInput = React.forwardRef<
  HTMLInputElement,
  React.InputHTMLAttributes<HTMLInputElement> & {
    disabledReason?: React.ReactNode;
  }
>(
  function FormInput(
    { className, disabled, disabledReason, ...rest },
    ref,
  ) {
    const input = (
      <input
        ref={ref}
        {...rest}
        disabled={disabled}
        aria-disabled={disabled || undefined}
        className={cn(
          'h-9 min-w-0 w-full rounded-md border border-bd-1 bg-bg-2 px-3 font-sans text-sm text-tx-0 placeholder:text-tx-3 focus:outline-none disabled:pointer-events-none disabled:cursor-not-allowed disabled:border-bd-0 disabled:bg-bg-3 disabled:text-tx-3 disabled:opacity-100 read-only:cursor-default read-only:border-bd-0 read-only:bg-bg-3 read-only:text-tx-2',
          className,
        )}
      />
    );
    return (
      <DisabledControl
        disabled={Boolean(disabled)}
        reason={disabledReason}
        className="w-full"
      >
        {input}
      </DisabledControl>
    );
  },
);

export const FormTextarea = React.forwardRef<
  HTMLTextAreaElement,
  React.TextareaHTMLAttributes<HTMLTextAreaElement> & {
    disabledReason?: React.ReactNode;
  }
>(function FormTextarea(
  { className, disabled, disabledReason, ...rest },
  ref,
) {
  const textarea = (
    <textarea
      ref={ref}
      {...rest}
      disabled={disabled}
      aria-disabled={disabled || undefined}
      className={cn(
        'min-h-24 rounded-md border border-bd-1 bg-bg-2 px-3 py-2.5 font-sans text-sm leading-relaxed text-tx-0 placeholder:text-tx-3 focus:outline-none disabled:pointer-events-none disabled:cursor-not-allowed disabled:border-bd-0 disabled:bg-bg-3 disabled:text-tx-3 disabled:opacity-100 read-only:cursor-default read-only:border-bd-0 read-only:bg-bg-3 read-only:text-tx-2',
        className,
      )}
    />
  );
  return (
    <DisabledControl
      disabled={Boolean(disabled)}
      reason={disabledReason}
      className="w-full"
    >
      {textarea}
    </DisabledControl>
  );
});

export interface FormSelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  disabledReason?: React.ReactNode;
}

export function FormSelect({
  value,
  onChange,
  options,
  className,
  placeholder,
  disabled,
  disabledReason,
  ariaLabel,
}: {
  value: string;
  onChange: (v: string) => void;
  options: Array<FormSelectOption | string>;
  className?: string;
  placeholder?: string;
  disabled?: boolean;
  disabledReason?: React.ReactNode;
  ariaLabel?: string;
}) {
  const emptyItemValue = React.useId();
  const hasEmptyOption = options.some((opt) => (typeof opt === 'string' ? opt : opt.value) === '');
  const radixValue = value === '' && hasEmptyOption ? emptyItemValue : value;

  return (
    <Select
      value={radixValue}
      onValueChange={(next) => onChange(next === emptyItemValue ? '' : next)}
      {...(disabled !== undefined ? { disabled } : {})}
    >
      <DisabledControl
        disabled={Boolean(disabled)}
        reason={disabledReason}
        className="w-full"
      >
        <SelectTrigger
          aria-label={ariaLabel}
          aria-disabled={disabled || undefined}
          className={cn(
            'h-9 rounded-md border-bd-1 bg-bg-2 px-3 font-sans text-sm text-tx-0 focus:outline-none disabled:border-bd-0 disabled:bg-bg-3 disabled:text-tx-3 disabled:opacity-100',
            className,
          )}
        >
          <span className="min-w-0 flex-1 truncate whitespace-nowrap text-left">
            <SelectValue placeholder={placeholder} />
          </span>
        </SelectTrigger>
      </DisabledControl>
      <SelectContent>
        {options.map((opt) => {
          const v = typeof opt === 'string' ? opt : opt.value;
          const l = typeof opt === 'string' ? opt : opt.label;
          const optionDisabled = typeof opt === 'string' ? false : Boolean(opt.disabled);
          const optionDisabledReason =
            typeof opt === 'string' ? undefined : opt.disabledReason;
          // Radix reserves an empty value for clearing the selection. Keep
          // callers' empty-string domain value, but never pass it to an item.
          const itemValue = v === '' ? emptyItemValue : v;
          const item = (
            <SelectItem key={itemValue} value={itemValue} className="font-sans text-xs">
              {l}
            </SelectItem>
          );
          if (!optionDisabled) return item;
          return (
            <Tooltip key={itemValue}>
              <TooltipTrigger asChild>
                <SelectItem
                  value={itemValue}
                  disabled
                  aria-disabled="true"
                  className="font-sans text-xs"
                >
                  {l}
                </SelectItem>
              </TooltipTrigger>
              {optionDisabledReason && (
                <TooltipContent side="right" className="max-w-xs leading-relaxed">
                  {optionDisabledReason}
                </TooltipContent>
              )}
            </Tooltip>
          );
        })}
      </SelectContent>
    </Select>
  );
}

export function FormChecklist<T extends string>({
  options,
  selected,
  onChange,
  disabled = false,
  disabledReason,
  className,
}: {
  options: Array<{ value: T; label: string; hint?: string }>;
  selected: T[];
  onChange: (next: T[]) => void;
  disabled?: boolean;
  disabledReason?: React.ReactNode;
  className?: string;
}) {
  const content = (
    <div
      className={cn('flex flex-col gap-1.5', className)}
      aria-disabled={disabled || undefined}
    >
      {options.map((opt) => {
        const checked = selected.includes(opt.value);
        const toggle = () => {
          if (disabled) return;
          onChange(
            checked
              ? selected.filter((v) => v !== opt.value)
              : [...selected, opt.value],
          );
        };
        return (
          <div
            key={opt.value}
            onClick={toggle}
            className={cn(
              'flex min-h-11 items-start gap-3 rounded-md border px-3 py-2.5 font-sans text-sm transition-colors',
              disabled
                ? 'cursor-not-allowed border-bd-0 bg-bg-2 text-tx-3'
                : 'cursor-pointer',
              checked
                ? 'border-indigo/45 bg-bg-2'
                : !disabled &&
                    'border-bd-0 bg-bg-1 hover:border-bd-2 hover:bg-bg-2',
            )}
          >
            <input
              type="checkbox"
              checked={checked}
              disabled={disabled}
              aria-disabled={disabled || undefined}
              aria-label={opt.label}
              onClick={(event) => event.stopPropagation()}
              onChange={(event) =>
                onChange(
                  event.currentTarget.checked
                    ? checked ? selected : [...selected, opt.value]
                    : selected.filter((v) => v !== opt.value),
                )
              }
              className="mt-0.5 h-4 w-4 shrink-0 cursor-pointer accent-indigo disabled:cursor-not-allowed"
            />
            <div className="min-w-0 flex-1">
              <div className="font-semibold text-tx-0">{opt.label}</div>
              {opt.hint && <div className="mt-1 text-xs text-tx-3">{opt.hint}</div>}
            </div>
          </div>
        );
      })}
    </div>
  );
  return (
    <DisabledControl
      disabled={disabled}
      reason={disabledReason}
      className="w-full"
    >
      {content}
    </DisabledControl>
  );
}

export function FormRadio<T extends string>({
  options,
  value,
  onChange,
  className,
}: {
  options: Array<{ value: T; label: string; hint?: string }>;
  value: T;
  onChange: (v: T) => void;
  className?: string;
}) {
  const baseId = React.useId();
  return (
    <RadioGroup value={value} onValueChange={(next) => onChange(next as T)} className={cn('flex flex-col gap-1.5', className)}>
      {options.map((opt) => {
        const checked = value === opt.value;
        const id = `${baseId}-${opt.value}`;
        return (
          <label
            key={opt.value}
            htmlFor={id}
            onClick={() => onChange(opt.value)}
            className={cn(
              'flex min-h-11 cursor-pointer items-start gap-3 rounded-md border px-3 py-2.5 font-sans text-sm transition-colors',
              checked
                ? 'border-indigo/45 bg-bg-2'
                : 'border-bd-0 bg-bg-1 hover:border-bd-2 hover:bg-bg-2',
            )}
          >
            <RadioGroupItem
              id={id}
              value={opt.value}
              className="mt-0.5"
              onClick={(event) => event.stopPropagation()}
            />
            <div className="min-w-0 flex-1">
              <div className="font-semibold text-tx-0">{opt.label}</div>
              {opt.hint && <div className="mt-1 text-xs text-tx-3">{opt.hint}</div>}
            </div>
          </label>
        );
      })}
    </RadioGroup>
  );
}

/**
 * FieldArray — the shared repeatable-rows primitive behind every
 * add/remove/reorder list in the app (alert thresholds, escalation steps and
 * targets, mute matchers, schedule rotations). Generalizes the inline matcher
 * editor and `WebhookHeadersEditor` row pattern.
 *
 * `renderItem` draws a single row's inputs and calls `setItem` with the
 * replacement value (works for objects and primitives alike). FieldArray owns
 * the surrounding flex row, the remove control, and — when `reorderable` —
 * the up/down controls.
 */
export function FieldArray<T>({
  items,
  onChange,
  renderItem,
  newItem,
  addLabel,
  removeLabel = 'Remove',
  reorderLabel = { up: 'Move up', down: 'Move down' },
  minItems = 0,
  reorderable = false,
  emptyHint,
  rowClassName,
}: {
  items: T[];
  onChange: (next: T[]) => void;
  renderItem: (item: T, index: number, setItem: (next: T) => void) => React.ReactNode;
  newItem: () => T;
  addLabel: string;
  removeLabel?: string;
  reorderLabel?: { up: string; down: string };
  /** Keep at least this many rows — the remove control hides at the floor. */
  minItems?: number;
  reorderable?: boolean;
  emptyHint?: React.ReactNode;
  rowClassName?: string;
}) {
  const setItem = (i: number, next: T) =>
    onChange(items.map((item, idx) => (idx === i ? next : item)));
  const remove = (i: number) => onChange(items.filter((_, idx) => idx !== i));
  const move = (i: number, delta: number) => {
    const j = i + delta;
    if (j < 0 || j >= items.length) return;
    const next = items.slice();
    const [moved] = next.splice(i, 1);
    next.splice(j, 0, moved as T);
    onChange(next);
  };
  return (
    <div className="flex flex-col gap-3">
      {items.length === 0 && emptyHint && (
        <div className="rounded-md border border-dashed border-bd-1 bg-bg-1 px-3 py-3 font-sans text-xs text-tx-3">
          {emptyHint}
        </div>
      )}
      {items.map((item, i) => (
        <div key={i} className={cn('flex items-start gap-3', rowClassName)}>
          <div className="min-w-0 flex-1">{renderItem(item, i, (next) => setItem(i, next))}</div>
          {reorderable && (
            <div className="flex shrink-0 flex-col">
              <button
                type="button"
                onClick={() => move(i, -1)}
                disabled={i === 0}
                aria-label={reorderLabel.up}
                title={reorderLabel.up}
                className="flex h-8 w-8 items-center justify-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0 disabled:pointer-events-none disabled:opacity-30"
              >
                <ChevronUp className="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                onClick={() => move(i, 1)}
                disabled={i === items.length - 1}
                aria-label={reorderLabel.down}
                title={reorderLabel.down}
                className="flex h-8 w-8 items-center justify-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0 disabled:pointer-events-none disabled:opacity-30"
              >
                <ChevronDown className="h-3.5 w-3.5" />
              </button>
            </div>
          )}
          {items.length > minItems && (
            <button
              type="button"
              onClick={() => remove(i)}
              aria-label={removeLabel}
              title={removeLabel}
              className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-red-soft"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      ))}
      <div>
        <ChromeButton type="button" onClick={() => onChange([...items, newItem()])}>
          <Plus className="h-3 w-3" /> {addLabel}
        </ChromeButton>
      </div>
    </div>
  );
}

export function FormSubmitFooter({
  busy,
  disabled,
  invalid,
  disabledReason,
  cancelDisabled,
  onCancel,
  submitLabel,
  formId,
}: {
  busy?: boolean;
  /** Permission or workflow state prevents submission. */
  disabled?: boolean;
  /** The current draft does not satisfy the form's validation rules. */
  invalid?: boolean;
  /** Explains a permission, license, or workflow restriction. */
  disabledReason?: React.ReactNode;
  cancelDisabled?: boolean;
  onCancel: () => void;
  submitLabel?: string;
  /** id of the <form> being submitted; allows the button to live outside the form */
  formId?: string;
}) {
  const { t } = useTranslation('common');
  // Type cast because React's HTMLButtonElement does support the `form` attribute
  // but our ChromeButton's prop typing doesn't surface it.
  const extra = formId ? { form: formId } : {};
  const submitDisabled = Boolean(busy || disabled || invalid);
  const submitDisabledReason =
    disabledReason ??
    (invalid
      ? t('access.form_invalid')
      : busy
        ? t('access.operation_pending')
        : undefined);
  return (
    <>
      <ChromeButton type="button" onClick={onCancel} disabled={cancelDisabled || busy}>
        {t('actions.cancel')}
      </ChromeButton>
      <ChromeButton
        type="submit"
        variant="primary"
        disabled={submitDisabled}
        disabledReason={submitDisabledReason}
        {...extra}
      >
        {busy ? t('status.saving') : (submitLabel ?? t('actions.save'))}
      </ChromeButton>
    </>
  );
}
