import { cn } from '@/shell/lib/cn';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

export function ControlSelect({
  value,
  onChange,
  options,
  className,
  ariaLabel,
}: {
  value: string;
  onChange: (v: string) => void;
  options: Array<{ value: string; label: string }>;
  className?: string;
  ariaLabel?: string;
}) {
  const selectedLabel =
    options.find((option) => option.value === value)?.label ?? value;
  return (
    <Select
      value={value}
      onValueChange={onChange}
    >
      <SelectTrigger
        aria-label={ariaLabel}
        className={cn(
          'h-10 w-auto max-w-[200px] shrink-0 rounded-md !border-0 bg-transparent px-2.5 font-sans text-xs font-strong text-tx-1 shadow-none outline-none hover:bg-bg-3 hover:text-tx-0 focus:outline-none focus:ring-0 focus:ring-offset-0 data-[state=open]:bg-bg-3 md:h-8 [&>span]:min-w-0 [&>span]:truncate [&>svg]:ml-1.5 [&>svg]:h-3.5 [&>svg]:w-3.5 [&>svg]:text-indigo [&>svg]:opacity-100',
          className,
        )}
      >
        <SelectValue>{selectedLabel}</SelectValue>
      </SelectTrigger>
      <SelectContent side="top" align="start" sideOffset={6} className="max-w-[min(360px,calc(100vw-32px))] !shadow-lg">
        {options.map((opt) => (
          <SelectItem key={opt.value} value={opt.value} className="font-sans text-xs">
            {opt.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
