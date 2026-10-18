import type { TFunction } from 'i18next';
import { CheckCircle2, ChevronDown, ChevronRight, CircleAlert, CircleGauge, Loader2 } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { activitySummary } from './activity';
import { parseJson, summarizeToolPayload } from './toolSummary';
import {
  fallbackToolLabel,
  formatInvestigationDuration,
  type InvestigationEvidenceItem,
  isRedundantInvestigationSummary,
} from '../../settings/presentation';
import type { LiveTool } from '../run/runState';

/**
 * What an answer did before it was written, in one line the way Claude shows
 * its work ("Looked at 3 items, read the knowledge base ›"); the calls one by
 * one when opened. It stands above the answer, where the work came.
 */
export function InvestigationProcess({
  t,
  evidence = [],
  liveTools = [],
  running = false,
  startedAtMicros,
  finishedAtMicros,
}: {
  t: TFunction<'agent'>;
  evidence?: InvestigationEvidenceItem[];
  liveTools?: LiveTool[];
  running?: boolean;
  startedAtMicros?: number;
  finishedAtMicros?: number;
}) {
  const [open, setOpen] = React.useState(false);
  const listId = React.useId();
  const liveEvidence: InvestigationEvidenceItem[] = liveTools.map((tool) => ({
    toolCallId: tool.id,
    tool: tool.name,
    status:
      tool.status === 'done'
        ? 'success'
        : tool.status === 'error'
          ? 'error'
          : 'running',
    summary:
      summarizeToolPayload(
        tool.name,
        tool.result,
        tool.status === 'error' ? 'error' : 'result',
      )[0] ?? '',
    ...(tool.arguments ? { arguments: parseJson(tool.arguments) ?? tool.arguments } : {}),
  }));
  const items = liveEvidence.length > 0 ? liveEvidence : evidence;
  if (items.length === 0) return null;
  const failed = items.filter((item) => item.status === 'error').length;
  const summary = [
    activitySummary(
      items.map((item) => item.tool),
      t,
    ),
    ...(failed > 0 ? [t('activity.failed', { count: failed })] : []),
  ].join(t('activity.separator'));
  const elapsedMs =
    startedAtMicros !== undefined && finishedAtMicros !== undefined
      ? Math.max(0, (finishedAtMicros - startedAtMicros) / 1000)
      : undefined;
  return (
    <div data-testid="investigation-process" data-state={open ? 'open' : 'closed'} className="mb-2">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen((current) => !current)}
        title={elapsedMs === undefined ? undefined : formatInvestigationDuration(elapsedMs)}
        className="-ml-1 inline-flex min-h-7 max-w-full items-center gap-1 rounded-md px-1 text-left font-sans text-tx-2 transition-colors duration-fast hover:text-tx-0 focus-visible:text-tx-0"
      >
        {running && (
          <Loader2 aria-hidden="true" className="h-3.5 w-3.5 shrink-0 animate-spin text-indigo" />
        )}
        <span aria-live="polite" className="min-w-0">
          {running ? `${t('activity.running')} · ${summary}` : summary}
        </span>
        <ChevronRight
          aria-hidden="true"
          className={cn(
            'h-3.5 w-3.5 shrink-0 transition-transform duration-fast',
            open && 'rotate-90',
          )}
        />
      </button>
      {open && (
        <div id={listId} className="mt-1 space-y-px border-l border-bd-0 pl-2">
          {items.map((item) => (
            <InvestigationProcessRow key={item.toolCallId} item={item} t={t} />
          ))}
        </div>
      )}
    </div>
  );
}

function InvestigationProcessRow({
  item,
  t,
}: {
  item: InvestigationEvidenceItem;
  t: (key: string) => string;
}) {
  const [open, setOpen] = React.useState(false);
  const detailId = React.useId();
  const Icon =
    item.status === 'success'
      ? CheckCircle2
      : item.status === 'error'
        ? CircleAlert
        : CircleGauge;
  const translationKey = `tools.${item.tool}.title`;
  const translated = t(translationKey);
  const displayName =
    translated === translationKey
      ? fallbackToolLabel(item.tool)
      : translated;
  const showSummary =
    item.summary &&
    item.summary !== 'ok' &&
    !isRedundantInvestigationSummary(item.summary, item.rowCount);
  // One line per call: the tool, what it returned and how long it took, with the
  // call itself one click away; a summary takes a second line.
  return (
    <div data-agent-process-row className="rounded-md px-2 py-1 hover:bg-bg-2">
      <div className="flex min-w-0 items-center gap-2">
        <Icon
          aria-hidden="true"
          className={cn(
            'h-3.5 w-3.5 shrink-0',
            item.status === 'success'
              ? 'text-green'
              : item.status === 'error'
                ? 'text-[var(--yellow-ink)]'
                : 'text-indigo',
          )}
        />
        <span className="min-w-0 truncate text-xs font-strong text-tx-1">{displayName}</span>
        {item.rowCount !== undefined && (
          <span className="type-micro shrink-0 text-tx-3">
            {t('investigation_process.rows').replace(
              '{{count}}',
              String(item.rowCount),
            )}
          </span>
        )}
        {item.tookMs !== undefined && (
          <span className="type-micro shrink-0 tabular-nums text-tx-3">{item.tookMs}ms</span>
        )}
        <button
          type="button"
          aria-expanded={open}
          aria-controls={detailId}
          onClick={() => setOpen((current) => !current)}
          className="type-micro ml-auto inline-flex shrink-0 items-center gap-0.5 rounded px-1 text-tx-3 hover:text-tx-1"
        >
          {t('investigation_process.technical_detail')}
          <ChevronDown
            aria-hidden="true"
            className={cn('h-3 w-3 transition-transform duration-fast', open && 'rotate-180')}
          />
        </button>
      </div>
      {showSummary && (
        <p className="ml-[22px] line-clamp-2 text-xs text-tx-3">{item.summary}</p>
      )}
      {open && (
        <div
          id={detailId}
          className="type-micro ml-[22px] mt-1 max-h-32 overflow-auto rounded bg-bg-3 px-2 py-1.5 font-mono text-tx-2"
        >
          <div>{item.tool}</div>
          {item.arguments !== undefined && (
            <pre className="whitespace-pre-wrap break-words">
              {JSON.stringify(item.arguments, null, 2)}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
