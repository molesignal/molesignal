import {
  ChartSpline,
  Flame,
  type LucideIcon,
  ScrollText,
  Share2,
  TableProperties,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { StreamType } from '@/api/streams';
import { cn } from '@/shell/lib/cn';

// The same marks the sidebar uses for each signal, so a tag reads as "that nav item".
// Keyed by string rather than `StreamType`: a type the client does not know yet reads as
// an extension table instead of taking the page down with it.
const TYPE_ICON: Record<string, LucideIcon> = {
  logs: ScrollText,
  metrics: ChartSpline,
  traces: Share2,
  profiles: Flame,
  extend: TableProperties,
};

const TYPE_LABEL_KEY: Record<string, string> = {
  logs: 'logs',
  metrics: 'metrics',
  traces: 'traces',
  profiles: 'profiles',
  extend: 'extend_tables',
};

/**
 * A stream's signal type as a neutral outline tag with the signal's own mark.
 * Color is reserved for status, so a type never competes with (or is mistaken
 * for) a health state. Used wherever a stream's type is shown: Home, the
 * Streams list and the stream detail header.
 */
export function StreamTypeTag({
  type,
  label,
  className,
}: {
  type: StreamType;
  /** Overrides the product name for the type (the default comes from the nav copy). */
  label?: string | undefined;
  className?: string | undefined;
}) {
  const { t } = useTranslation('nav');
  const Icon = TYPE_ICON[type] ?? TableProperties;
  return (
    <span
      className={cn(
        'inline-flex h-[22px] items-center gap-1.5 whitespace-nowrap rounded-md border border-bd-1 px-2 font-sans text-xs font-medium text-tx-1',
        className,
      )}
    >
      <Icon aria-hidden="true" className="h-[13px] w-[13px] text-tx-2" />
      {label ?? t(TYPE_LABEL_KEY[type] ?? 'extend_tables')}
    </span>
  );
}
