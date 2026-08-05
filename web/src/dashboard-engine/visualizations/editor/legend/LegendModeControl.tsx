import { cn } from '@/shell/lib/cn';

export type LegendMode = 'list' | 'table' | 'hidden';

const LEGEND_MODES = ['list', 'table', 'hidden'] as const;

export function LegendModeControl({
  value,
  label,
  optionLabel,
  onChange,
}: {
  value: LegendMode;
  label: string;
  optionLabel: (value: LegendMode) => string;
  onChange: (value: LegendMode) => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="grid h-8 grid-cols-3 overflow-hidden rounded-md border border-bd-1 bg-bg-1"
    >
      {LEGEND_MODES.map((mode) => {
        const selected = value === mode;
        const text = optionLabel(mode);
        return (
          <button
            key={mode}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={text}
            onClick={() => onChange(mode)}
            className={cn(
              'min-w-0 truncate border-r border-bd-0 px-1 font-sans text-type-micro font-semibold last:border-r-0',
              selected
                ? 'bg-indigo-dim text-indigo-soft'
                : 'text-tx-2 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0',
            )}
          >
            {text}
          </button>
        );
      })}
    </div>
  );
}
