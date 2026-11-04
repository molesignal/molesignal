import * as React from 'react';

import { Switch } from '@/shell/ui/switch';

/** A setting that is on or off: what it is, what it does, and the switch. */
export function SettingSwitch({
  label,
  hint,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  hint: string;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  const id = React.useId();

  return (
    <div className="flex items-start justify-between gap-4">
      <div className="min-w-0">
        <label htmlFor={id} className="block font-sans text-sm font-strong text-tx-0">
          {label}
        </label>
        <p className="m-0 mt-0.5 font-sans text-xs leading-relaxed text-tx-3">{hint}</p>
      </div>
      <Switch id={id} checked={checked} disabled={disabled} onCheckedChange={onChange} />
    </div>
  );
}
