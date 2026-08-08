import { BarChart3 } from 'lucide-react';

import { QueryToolbarButton } from '@/shell/query/Workbench';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

interface HistogramToggleProps {
  visible: boolean;
  label: string;
  onVisibleChange: (visible: boolean) => void;
}

export function HistogramToggle({
  visible,
  label,
  onVisibleChange,
}: HistogramToggleProps) {
  return (
    <TooltipProvider delayDuration={250}>
      <Tooltip>
        <TooltipTrigger asChild>
          <QueryToolbarButton
            active={visible}
            tone="blue"
            aria-label={label}
            aria-pressed={visible}
            onClick={() => onVisibleChange(!visible)}
            className="w-9 px-0"
          >
            <BarChart3 aria-hidden="true" className="h-4 w-4" />
          </QueryToolbarButton>
        </TooltipTrigger>
        <TooltipContent>{label}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
