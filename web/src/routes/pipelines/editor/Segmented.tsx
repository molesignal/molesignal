import { cn } from '@/shell/lib/cn';

/**
 * A choice between a few named options that are all on view: how a pipeline
 * runs, where a route sends events. The option in force is a lighter key on
 * the control's fill; nothing here is drawn with a border.
 */
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
  disabled = false,
  className,
}: {
  /** Names the group for assistive technology; the visible caption is the caller's. */
  label: string;
  value: T;
  options: ReadonlyArray<{ value: T; label: string }>;
  onChange: (value: T) => void;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <div
      role="group"
      aria-label={label}
      className={cn('flex h-9 rounded-md bg-[var(--control-surface)] p-[3px]', className)}
    >
      {options.map((option) => {
        const selected = option.value === value;

        return (
          <button
            key={option.value}
            type="button"
            disabled={disabled}
            aria-pressed={selected}
            onClick={() => onChange(option.value)}
            className={cn(
              'min-w-0 flex-1 truncate rounded px-3 font-sans text-xs outline-none transition-colors duration-fast disabled:cursor-not-allowed disabled:text-tx-3',
              selected
                ? 'bg-[var(--functional-surface)] font-strong text-tx-0'
                : 'text-tx-2 enabled:hover:bg-bg-3 enabled:hover:text-tx-0 enabled:focus-visible:bg-bg-3 enabled:focus-visible:text-tx-0',
            )}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
