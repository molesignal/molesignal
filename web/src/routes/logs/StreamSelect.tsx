import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { StreamSummary } from '@/api/streams';
import { sizingLabels } from '@/shell/ui/fitted-select';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

/** The names the field must be wide enough for, longest first. */
export const sizingNames = sizingLabels;

/**
 * Says a name in full under the pointer where it is cut short, and only there.
 * The field holds the chosen row's own name inside its own, so either may be
 * the one that is cut.
 */
function tellWhenCut(event: React.PointerEvent<HTMLElement>) {
  const names = [...event.currentTarget.querySelectorAll<HTMLElement>('[data-log-stream-name]')];
  const cut = names.some((name) => name.scrollWidth > name.clientWidth);

  if (cut) event.currentTarget.title = names[0]?.textContent ?? '';
  else event.currentTarget.removeAttribute('title');
}

/**
 * Which log stream the explorer queries; a placeholder holds its place until
 * streams load.
 *
 * The field is as wide as the longest name of the list, so it gives the query
 * beside it the room short names leave and does not change width when another
 * stream is chosen. It stops at 220px, the width it had while it was fixed: a
 * name longer than that is cut with an ellipsis, in the field and in the list,
 * and said in full under the pointer. The list opens exactly as wide as the
 * field.
 */
export function LogStreamSelect({
  streams,
  value,
  loading,
  onChange,
}: {
  streams: readonly StreamSummary[];
  value: string;
  loading: boolean;
  onChange: (stream: string) => void;
}) {
  const { t } = useTranslation('logs');
  const widest = React.useMemo(
    () => sizingNames(streams.map((item) => item.label || item.name)),
    [streams],
  );

  if (streams.length === 0) {
    return (
      <div className="flex h-9 max-w-[220px] items-center rounded-md bg-[var(--control-surface)] px-2.5 font-sans text-xs font-strong text-tx-2">
        <span className="truncate">
          {loading ? t('explore.left_panel.loading_streams') : t('explore.left_panel.no_streams')}
        </span>
      </div>
    );
  }

  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger
        aria-label={t('explore.left_panel.select_stream_aria')}
        data-log-stream-select
        onPointerEnter={tellWhenCut}
        // A row of the list is its name with 48px around it (the list's edge, the tick before the
        // name, the row's end). The field has the same around its longest name and 1px to spare,
        // its arrow and the gap before it included, so no row is cut while the field is not.
        className="h-9 w-auto max-w-[220px] shrink-0 gap-[calc(35px-1.25rem)] rounded-md border-0 bg-[var(--control-surface)] px-2.5 font-sans text-xs font-strong text-tx-0 shadow-none"
      >
        {/* One cell holds the chosen name and, unseen, the longest names of the list: it is as wide as the widest. */}
        <span className="grid min-w-0 text-left">
          <span data-log-stream-name className="col-start-1 row-start-1 truncate">
            <SelectValue placeholder={t('explore.left_panel.select_stream_placeholder')} />
          </span>
          {widest.map((name) => (
            <span
              key={name}
              aria-hidden="true"
              data-label={name}
              className="invisible col-start-1 row-start-1 h-0 overflow-hidden whitespace-nowrap before:content-[attr(data-label)]"
            />
          ))}
        </span>
      </SelectTrigger>
      <SelectContent data-log-stream-list className="w-[var(--radix-select-trigger-width)] min-w-0">
        {streams.map((item) => (
          <SelectItem
            key={item.id}
            value={item.name}
            onPointerEnter={tellWhenCut}
            // The name is the row's only part in the flow: it gives way, so a long one is cut, not the row.
            className="font-sans text-xs [&>span:last-child]:min-w-0"
          >
            <span data-log-stream-name className="block truncate text-tx-0">
              {item.label || item.name}
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
