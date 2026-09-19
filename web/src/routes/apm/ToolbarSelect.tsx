import { cn } from '@/shell/lib/cn';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

export interface ApmToolbarSelectOption {
  value: string;
  label: string;
}

const EMPTY_OPTION_VALUE = '__molesignal_empty_option__';

export function ApmToolbarSelect({
  ariaLabel,
  value,
  options,
  className,
  onValueChange,
}: {
  ariaLabel: string;
  value: string;
  options: readonly ApmToolbarSelectOption[];
  className?: string;
  onValueChange: (value: string) => void;
}) {
  return (
    <Select
      value={value || EMPTY_OPTION_VALUE}
      onValueChange={(nextValue) =>
        onValueChange(nextValue === EMPTY_OPTION_VALUE ? '' : nextValue)
      }
    >
      <SelectTrigger
        aria-label={ariaLabel}
        className={cn(
          'h-[44px] w-auto min-w-[112px] rounded-sm border-0 bg-[var(--control-surface)] px-[10px] py-0 text-xs font-medium text-tx-1 shadow-none hover:bg-bg-3 hover:text-tx-0 data-[state=open]:border-0 sm:h-[30px]',
          className,
        )}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent align="start">
        {options.map((option) => (
          <SelectItem
            key={option.value || EMPTY_OPTION_VALUE}
            value={option.value || EMPTY_OPTION_VALUE}
          >
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
