import { Share2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'react-router-dom';

import { writeClipboardText } from '@/lib/clipboard';
import { useTimeFormatter } from '@/lib/time';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { QueryIconAction } from '@/shell/query/IconAction';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';
import { RadioGroup, RadioGroupItem } from '@/shell/ui/radio-group';
import { toast } from '@/shell/ui/sonner';
import { useFiltersStore } from '@/stores/useFiltersStore';
import { useTimeStore } from '@/stores/useTimeStore';
import { formatTimeWindowLabel } from '@/time/TimePicker';

import { fixedWindow, queryShareLink } from './link';

/** How long the copy button says that it has copied. */
const COPIED_MS = 2_000;
const OPTION =
  'flex cursor-pointer items-start gap-2 rounded-md px-2 py-1.5 hover:bg-bg-3 has-[:focus-visible]:bg-bg-3';

interface QueryShareButtonProps {
  /** The query on screen, as the page's own search params say it. */
  search: URLSearchParams;
}

/**
 * Shares the query an explorer is showing, as a link: the page's address with
 * the query, the time range and the pinned filters written into it. Whoever
 * opens it is signed in and reads the data with their own rights; nothing is
 * stored and nothing is made public.
 *
 * A time range that moves ("the last hour") is fixed to the moments the
 * current selection covers when the reader asks to share, unless they ask to
 * keep it moving: a link is mostly sent to show what was seen.
 */
export function QueryShareButton({ search }: QueryShareButtonProps) {
  const { t } = useTranslation('common');
  const { pathname } = useLocation();
  const format = useTimeFormatter();
  const timeWindow = useTimeStore((state) => state.window);
  const anchor = useTimeStore((state) => state.anchor);
  const filters = useFiltersStore((state) => state.filters);
  const [open, setOpen] = React.useState(false);
  const [time, setTime] = React.useState<'fixed' | 'moving'>('fixed');
  const [copied, setCopied] = React.useState(false);
  // The moments are those of the instant the reader asked to share: the link does not change while it is read.
  const [askedAt, setAskedAt] = React.useState(() => new Date());
  const titleId = React.useId();

  const moves = timeWindow.mode === 'relative';
  const moments = fixedWindow(timeWindow, askedAt);
  const link = queryShareLink(
    { pathname, own: search, window: timeWindow, anchor, filters },
    { fixed: !moves || time === 'fixed', origin: globalThis.location.origin, now: askedAt },
  );
  const span = `${format.micros(Date.parse(moments.from) * 1_000)} – ${format.micros(Date.parse(moments.to) * 1_000)}`;

  React.useEffect(() => {
    if (!copied) return undefined;

    const timer = globalThis.setTimeout(() => setCopied(false), COPIED_MS);

    return () => globalThis.clearTimeout(timer);
  }, [copied]);

  const copy = async () => {
    try {
      await writeClipboardText(link);
      setCopied(true);
    } catch {
      toast.error(t('share_query.copy_failed'));
    }
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);

        if (next) {
          setAskedAt(new Date());
          setCopied(false);
        }
      }}
    >
      <PopoverTrigger asChild>
        <QueryIconAction data-query-share icon={Share2} label={t('share_query.button')} />
      </PopoverTrigger>
      <PopoverContent
        align="end"
        data-query-share-panel
        aria-labelledby={titleId}
        className="w-[380px] max-w-[calc(100vw-24px)] p-4 font-sans"
      >
        <h2 id={titleId} className="text-sm font-strong text-tx-0">
          {t('share_query.title')}
        </h2>
        <p className="mt-1 text-xs leading-relaxed text-tx-2">{t('share_query.description')}</p>

        {moves ? (
          <RadioGroup
            value={time}
            onValueChange={(value) => {
              setTime(value === 'moving' ? 'moving' : 'fixed');
              setCopied(false);
            }}
            aria-label={t('share_query.time_label')}
            className="-mx-2 mt-3 gap-0.5"
          >
            <label className={OPTION}>
              <RadioGroupItem value="fixed" className="mt-[2px] shrink-0" />
              <span className="min-w-0 text-xs">
                <span className="block font-body text-tx-0">{t('share_query.time_fixed')}</span>
                <span data-query-share-moments className="block tabular-nums text-tx-2">
                  {span}
                </span>
              </span>
            </label>
            <label className={OPTION}>
              <RadioGroupItem value="moving" className="mt-[2px] shrink-0" />
              <span className="min-w-0 text-xs font-body text-tx-0">
                {t('share_query.time_moving', { range: formatTimeWindowLabel(timeWindow, t) })}
              </span>
            </label>
          </RadioGroup>
        ) : (
          <p data-query-share-moments className="mt-3 text-xs tabular-nums text-tx-1">
            {span}
          </p>
        )}

        <div className="mt-3 flex items-center gap-1 rounded-md bg-[var(--control-surface)] pl-2.5 pr-1">
          <input
            readOnly
            value={link}
            aria-label={t('share_query.link_aria')}
            onFocus={(event) => event.currentTarget.select()}
            className="h-9 min-w-0 flex-1 bg-transparent font-sans text-xs text-tx-1 focus:outline-none"
          />
          <CopyIconButton
            label={t('share_query.copy')}
            copied={copied}
            copiedLabel={t('share_query.copied')}
            onClick={() => void copy()}
          />
        </div>
      </PopoverContent>
    </Popover>
  );
}
