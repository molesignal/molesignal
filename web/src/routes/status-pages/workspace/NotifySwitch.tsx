import * as React from 'react';

import { uiLabelClass } from '@/shell/chrome';
import { Switch } from '@/shell/ui/switch';

/**
 * Whether what is about to change on the page is also sent to its
 * subscribers. Off, only the page changes: a correction of wording or a
 * status set right after the fact need not write to anyone.
 */
export function NotifySwitch({
  label,
  hint,
  checked,
  onCheckedChange,
  disabled,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  const id = React.useId();

  return (
    <div className="flex items-start justify-between gap-4">
      <div className="min-w-0">
        <label htmlFor={id} className={uiLabelClass}>
          {label}
        </label>
        {hint && <p className="mt-1 text-xs leading-relaxed text-tx-3">{hint}</p>}
      </div>
      <Switch
        id={id}
        checked={checked}
        onCheckedChange={onCheckedChange}
        disabled={disabled}
        aria-label={label}
        className="mt-0.5"
      />
    </div>
  );
}
