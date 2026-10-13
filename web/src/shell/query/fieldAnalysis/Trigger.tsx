import { ChartLine } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

interface FieldAnalysisTriggerProps {
  /** The field the analysis would be of. */
  name: string;
  /** Its analysis is the one that is open: the button closes it again. */
  pressed: boolean;
  onPress: () => void;
  /** The box of a row action, which the row decides. */
  className?: string | undefined;
}

/**
 * The button of a field's row that opens what the field holds across the
 * results: an action beside the row's others, lit while its panel is open.
 */
export function FieldAnalysisTrigger({ name, pressed, onPress, className }: FieldAnalysisTriggerProps) {
  const { t } = useTranslation('common');
  const label = t('field_analysis.open_aria', { name });

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          onClick={onPress}
          aria-label={label}
          aria-pressed={pressed}
          data-field-analysis-trigger={name}
          className={cn(
            className,
            pressed
              ? 'bg-[var(--blue-tint)] text-[var(--blue-ink)]'
              : 'hover:text-[var(--blue-ink)] focus-visible:text-[var(--blue-ink)]',
          )}
        >
          <ChartLine className="h-3 w-3" aria-hidden="true" />
        </button>
      </TooltipTrigger>
      <TooltipContent>{t(pressed ? 'field_analysis.close' : 'field_analysis.open')}</TooltipContent>
    </Tooltip>
  );
}
