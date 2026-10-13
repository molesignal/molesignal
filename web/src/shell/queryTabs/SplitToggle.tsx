import { Columns2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

interface SplitToggleProps {
  /** The page is split in two halves now. */
  split: boolean;
  /** The window has room for two halves. */
  room: boolean;
  onChange: (split: boolean) => void;
}

/**
 * Splits the page in two halves, each as wide as the other and each with its
 * own tabs over it, or gives the page back to one. It stands at the far end of
 * the tabs. A window too narrow for two halves has no switch.
 */
export function SplitToggle({ split, room, onChange }: SplitToggleProps) {
  const { t } = useTranslation('common');

  if (!room && !split) return null;

  const name = t(split ? 'query_tabs.unsplit' : 'query_tabs.split');

  return (
    <button
      type="button"
      aria-label={name}
      aria-pressed={split}
      title={name}
      data-query-split={split ? 'on' : 'off'}
      onClick={() => onChange(!split)}
      className={cn(
        'ms-press grid h-[28px] w-[28px] shrink-0 place-items-center rounded-md transition-colors duration-fast',
        split
          ? 'bg-bg-3 text-tx-0'
          : 'text-tx-2 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0',
      )}
    >
      <Columns2 className="h-[14px] w-[14px]" aria-hidden="true" />
    </button>
  );
}
