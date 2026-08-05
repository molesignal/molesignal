import { Upload } from 'lucide-react';
import * as React from 'react';

import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';

export function FilePicker({
  id,
  label,
  buttonLabel,
  fileName,
  accept,
  required,
  disabled = false,
  disabledReason,
  className,
  onFile,
}: {
  id?: string;
  label?: React.ReactNode;
  buttonLabel: React.ReactNode;
  fileName?: string | undefined;
  accept?: string;
  required?: boolean;
  disabled?: boolean;
  disabledReason?: React.ReactNode;
  className?: string;
  onFile: (file: File) => void | Promise<void>;
}) {
  const reactId = React.useId();
  const inputId = id ?? reactId;

  const handleChange = async (event: React.ChangeEvent<HTMLInputElement>) => {
    if (disabled) return;
    const file = event.target.files?.[0];
    if (file) {
      await onFile(file);
    }
    event.target.value = '';
  };

  return (
    <div className={cn('flex flex-wrap items-center gap-2', className)}>
      {label && <span className="font-sans text-xs text-tx-2">{label}</span>}
      <DisabledControl disabled={disabled} reason={disabledReason}>
        <label
          htmlFor={disabled ? undefined : inputId}
          aria-disabled={disabled || undefined}
          className={cn(
            'inline-flex h-[26px] items-center gap-1.5 rounded-md border border-bd-1 bg-bg-2 px-2.5',
            'font-sans text-xs font-strong text-tx-1 transition-colors',
            disabled
              ? 'pointer-events-none cursor-not-allowed border-bd-0 text-tx-3'
              : 'cursor-pointer hover:border-bd-2 hover:bg-bg-3 hover:text-tx-0',
            'focus-within:outline-none',
          )}
        >
          <Upload className="h-3.5 w-3.5" />
          <span>{buttonLabel}</span>
          <input
            id={inputId}
            type="file"
            accept={accept}
            required={required}
            disabled={disabled}
            aria-disabled={disabled || undefined}
            onChange={handleChange}
            className="sr-only"
          />
        </label>
      </DisabledControl>
      {fileName && (
        <span className="min-w-0 max-w-[320px] truncate rounded-md border border-bd-0 bg-bg-2 px-2 py-1 font-sans text-xs text-tx-2">
          {fileName}
        </span>
      )}
    </div>
  );
}
