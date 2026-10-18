import { Switch } from '@/shell/ui/switch';

/** A switch with its label and a line of hint, on a quiet fill. */
export function ToggleField({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="flex items-center justify-between gap-4 rounded-md bg-bg-2 px-3 py-2.5">
      <span>
        <span className="block text-xs font-strong text-tx-1">{label}</span>
        {hint && <span className="mt-0.5 block text-xs text-tx-3">{hint}</span>}
      </span>
      <Switch checked={checked} onCheckedChange={onChange} />
    </label>
  );
}
