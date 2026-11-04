import { Layers } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumFrame, RumFramePlace, RumIssueStack } from '@/api/rum';
import { cn } from '@/shell/lib/cn';

import { OverviewCard } from '../../overview/OverviewCard';

type View = 'restored' | 'raw';

/** `file:line:column` of a frame, as far as it has them. */
function whereIs(place: RumFramePlace): string {
  const line = place.line === null ? '' : `:${place.line}`;
  const column = place.line !== null && place.column !== null ? `:${place.column}` : '';

  return `${place.file}${line}${column}`;
}

/** What a frame is shown as: the restored place when that is what is looked at and the frame has one. */
function shown(frame: RumFrame, view: View): { place: RumFramePlace; restored: boolean } {
  return view === 'restored' && frame.restored
    ? { place: frame.restored, restored: true }
    : { place: frame.raw, restored: false };
}

/**
 * Where the error was thrown: the frames of its stack, the application's own in strong type and the
 * libraries' in a quiet one. When a source map, a mapping or a symbol file resolved the frames,
 * they are shown as resolved and the page can be switched to what the SDK sent.
 */
export function StackCard({ stack }: { stack: RumIssueStack | null }) {
  const { t } = useTranslation('rum');
  const frames = stack?.frames ?? [];
  const hasRestored = frames.some((frame) => frame.restored !== null);
  const [chosen, setChosen] = React.useState<View | null>(null);
  const view: View = chosen ?? (hasRestored ? 'restored' : 'raw');

  const toggle = hasRestored && (
    <div role="radiogroup" aria-label={t('errors.stack.view')} className="ml-auto inline-flex gap-0.5 rounded-md bg-[var(--control-surface)] p-0.5">
      {(['restored', 'raw'] as const).map((option) => (
        <button
          key={option}
          type="button"
          role="radio"
          aria-checked={view === option}
          data-rum-stack-view={option}
          onClick={() => setChosen(option)}
          className={cn(
            'h-6 rounded-[5px] border-0 px-2.5 font-sans text-xs font-strong outline-none',
            view === option ? 'bg-bg-3 text-tx-0' : 'bg-transparent text-tx-2 hover:text-tx-0 focus-visible:text-tx-0',
          )}
        >
          {t(`errors.stack.${option}`)}
        </button>
      ))}
    </div>
  );

  const symbolication = stack?.symbolication;

  return (
    <OverviewCard
      icon={Layers}
      tone="text-[var(--chart-2)]"
      title={t('errors.stack.title')}
      description={
        frames.length === 0
          ? undefined
          : symbolication
            ? t(`errors.stack.symbolication.${symbolication}`)
            : t('errors.stack.symbolication.none')
      }
      testId="issue-stack"
    >
      {toggle && <div className="flex px-3 pt-2">{toggle}</div>}
      {frames.length > 0 ? (
        <ol data-rum-frames="" className="m-0 list-none p-0 pt-1">
          {frames.map((frame, index) => {
            const { place, restored } = shown(frame, view);

            return (
              <li
                key={`${index}:${place.file}:${place.function}`}
                data-rum-frame={frame.inApp ? 'app' : 'library'}
                className="flex items-center gap-3 border-b border-[var(--row-rule)] px-3 py-1.5 font-sans text-sm"
              >
                <span className={cn('min-w-0 flex-1 truncate', frame.inApp ? 'font-strong text-tx-0' : 'text-tx-3')}>
                  {place.function || t('errors.stack.anonymous')}
                </span>
                <span className={cn('min-w-0 flex-[2] truncate font-mono text-xs', frame.inApp ? 'text-tx-1' : 'text-tx-3')}>
                  {whereIs(place)}
                </span>
                <span className={cn('w-14 shrink-0 text-right text-xs', restored ? 'text-[var(--green-ink)]' : 'text-tx-3')}>
                  {frame.inApp ? (restored ? t('errors.stack.restored_tag') : '') : t('errors.stack.library')}
                </span>
              </li>
            );
          })}
        </ol>
      ) : stack?.text ? (
        <pre data-rum-stack-text="" className="m-0 max-h-64 overflow-auto whitespace-pre-wrap px-3 py-2 font-mono text-xs text-tx-1">
          {stack.text}
        </pre>
      ) : (
        <p className="m-0 px-3 py-3 font-sans text-sm text-tx-3">{t('errors.stack.empty')}</p>
      )}
    </OverviewCard>
  );
}
