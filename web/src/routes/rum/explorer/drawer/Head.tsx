import { ChevronDown, ChevronUp, ExternalLink, type LucideIcon, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumRow } from '@/api/rum';
import { writeClipboardText } from '@/lib/clipboard';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

/** A button of the head as a log's drawer has them: a 14px mark in the grey of the head, darker under the pointer. */
const HEAD_BUTTON =
  'grid h-8 w-8 place-items-center rounded-md text-tx-2 outline-none hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent disabled:hover:text-tx-2';

/** The neighbours of the row in the list it was opened from, and where it stands in it. */
export interface DrawerNeighbours {
  onPrevious?: (() => void) | undefined;
  onNext?: (() => void) | undefined;
  position?: { current: number; total: number } | undefined;
}

/**
 * The head of a row's drawer: what the row is called, its type and when it happened; then the way
 * to its session, the rows before and after it, its JSON to copy and the way out, as the head of a
 * log's drawer is laid out.
 */
export function RumDrawerHead({
  title,
  kind,
  moment,
  row,
  href,
  neighbours,
  onClose,
}: {
  title: string;
  kind: string;
  moment: string;
  row: RumRow;
  /** The page of the session the row belongs to. */
  href: string | null;
  neighbours: DrawerNeighbours;
  onClose: () => void;
}) {
  const { t } = useTranslation('rum');
  const [copied, setCopied] = React.useState(false);
  const timer = React.useRef<number | null>(null);
  const { position } = neighbours;

  React.useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );

  const copy = async () => {
    await writeClipboardText(JSON.stringify(row, null, 2));
    setCopied(true);
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(false), 1400);
  };

  return (
    <header className="flex min-h-[56px] shrink-0 items-center gap-3 py-2 pl-4 pr-2">
      <div className="min-w-0 flex-1">
        <p className="m-0 truncate font-sans text-sm font-strong text-tx-0" title={title}>
          {title}
        </p>
        <p className="m-0 truncate font-sans text-xs text-tx-3">
          {kind} · <span className="tabular-nums">{moment}</span>
        </p>
      </div>
      <div className="ml-auto flex shrink-0 items-center">
        {href && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Link to={href} aria-label={t('explorer.drawer.open_session')} data-testid="rum-drawer-session" className={HEAD_BUTTON}>
                <ExternalLink className="h-3.5 w-3.5" aria-hidden="true" />
              </Link>
            </TooltipTrigger>
            <TooltipContent>{t('explorer.drawer.open_session')}</TooltipContent>
          </Tooltip>
        )}
        <HeadButton label={t('explorer.drawer.previous')} icon={ChevronUp} onClick={neighbours.onPrevious} />
        {position && (
          <span
            aria-label={t('explorer.drawer.position', position)}
            className="min-w-[44px] px-[2px] text-center font-mono text-xs tabular-nums text-tx-2"
          >
            {position.current} / {position.total}
          </span>
        )}
        <HeadButton label={t('explorer.drawer.next')} icon={ChevronDown} onClick={neighbours.onNext} />
        <span aria-hidden="true" className="mx-[6px] h-4 w-px bg-bd-1" />
        <CopyIconButton
          label={t('explorer.drawer.copy_json')}
          copied={copied}
          copiedLabel={t('explorer.drawer.copied_json')}
          onClick={() => void copy()}
        />
        <HeadButton label={t('explorer.drawer.close')} icon={X} onClick={onClose} />
      </div>
    </header>
  );
}

function HeadButton({ label, icon: Icon, onClick }: { label: string; icon: LucideIcon; onClick?: (() => void) | undefined }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button type="button" aria-label={label} disabled={!onClick} onClick={onClick} className={HEAD_BUTTON}>
          <Icon className="h-3.5 w-3.5" aria-hidden="true" />
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
