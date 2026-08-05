import { uiLabelClass } from '@/shell/chrome';

import { CopyableValue } from '../_atoms';

interface CallbackUrlFieldProps {
  label: string;
  hint: string;
  value: string;
  copyLabel: string;
  copiedLabel: string;
}

/** Fixed service-provider callback shown for registration at the IdP. */
export function CallbackUrlField({
  label,
  hint,
  value,
  copyLabel,
  copiedLabel,
}: CallbackUrlFieldProps) {
  return (
    <div className="flex min-w-0 cursor-default flex-col gap-1.5">
      <span className={uiLabelClass}>{label}</span>
      <CopyableValue
        value={value}
        copyLabel={copyLabel}
        copiedLabel={copiedLabel}
      />
      <span className="font-sans text-xs leading-relaxed text-tx-3">
        {hint}
      </span>
    </div>
  );
}
