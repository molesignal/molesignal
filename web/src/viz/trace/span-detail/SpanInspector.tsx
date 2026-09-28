import { X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { Span } from '@/api/web';
import { writeClipboardText } from '@/lib/clipboard';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { SignalReference, type SignalReferenceTime } from '@/shell/SignalReference';
import { Button } from '@/shell/ui/button';

import { formatTraceDurationNs } from '../duration';
import { TraceOperationName } from '../TraceOperationName';
import { formatJson, pct, signalLabelsForSpan } from '../waterfallModel';
import { Attributes } from './Attributes';

export function SpanInspector({
  traceId,
  span,
  durationNs,
  startOffsetNs,
  totalNs,
  time,
  onClose,
}: {
  traceId: string;
  span: Span;
  durationNs: number;
  startOffsetNs: number;
  totalNs: number;
  time?: SignalReferenceTime | undefined;
  onClose: () => void;
}) {
  const { t } = useTranslation('traces');
  const labels = signalLabelsForSpan(traceId, span);
  const source = { type: 'trace' as const, id: traceId };
  return (
    <aside className="flex min-h-0 min-w-0 w-[380px] max-w-full shrink-0 flex-col border-l border-bd-0 bg-bg-0">
      <div className="flex items-start gap-3 border-b border-bd-0 px-4 py-3">
        <div className="min-w-0 flex-1">
          <SignalReference
            type="span_id"
            value={span.span_id}
            labelName="span_id"
            labels={labels}
            time={time}
            source={source}
            showIcon={false}
            className="max-w-full justify-start text-left text-tx-0 decoration-current/30"
          >
            <TraceOperationName
              operation={span.operation}
              className="font-sans text-sm font-strong text-tx-0"
            />
          </SignalReference>
          <div className="mt-0.5 flex flex-wrap gap-3 font-sans text-xs text-tx-2">
            <SignalReference
              type="service"
              value={span.service}
              labelName="service.name"
              labels={labels}
              time={time}
              source={source}
              showIcon={false}
              className="text-tx-2 decoration-current/30 hover:text-indigo-soft"
            >
              {span.service}
            </SignalReference>
            <span>{formatTraceDurationNs(durationNs)}</span>
            <span>start {formatTraceDurationNs(startOffsetNs)}</span>
            <span>{pct(startOffsetNs, totalNs).toFixed(1)}% into trace</span>
          </div>
        </div>
        <Button variant="ghost" size="icon" className="h-7 w-7 shrink-0" onClick={onClose} aria-label="Close span details">
          <X className="h-3.5 w-3.5" />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto">
        <div className="border-b border-bd-0 px-4 py-3">
          <div className="mb-2 font-sans text-xs font-strong tracking-normal text-tx-2">
            Span ID
          </div>
          <div className="flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded border border-bd-0 bg-bg-1 px-2 py-1 font-mono text-xs text-tx-1">
              {span.span_id}
            </code>
            <CopyIconButton
              label={t('detail.copy_span_id')}
              onClick={() => {
                void writeClipboardText(span.span_id);
              }}
            />
          </div>
        </div>
        <div className="grid grid-cols-3 border-b border-bd-0">
          <DetailMetric label="Duration" value={formatTraceDurationNs(durationNs)} />
          <DetailMetric label="Start" value={formatTraceDurationNs(startOffsetNs)} />
          <DetailMetric label="Offset" value={`${pct(startOffsetNs, totalNs).toFixed(1)}%`} />
        </div>
        <Attributes key={span.span_id} attributes={span.attributes} />
        <div className="min-w-0 p-4">
          <div className="mb-2 font-sans text-xs font-strong tracking-normal text-tx-2">
            Events
          </div>
          {span.events.length === 0 ? (
            <div className="rounded-md border border-dashed border-bd-1 bg-bg-1 p-3 font-sans text-xs text-tx-3">
              No span events.
            </div>
          ) : (
            <div className="space-y-2">
              {span.events.map((event, index) => (
                <div key={`${event.name}-${index}`} className="rounded-md border border-bd-0 bg-bg-1 p-2">
                  <div className="font-sans text-xs font-strong text-tx-0">{event.name}</div>
                  <pre className="mt-1 whitespace-pre-wrap break-words [overflow-wrap:anywhere] font-mono text-xs leading-4 text-tx-2">
                    {formatJson(event.attributes)}
                  </pre>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </aside>
  );
}

function DetailMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 border-r border-bd-0 px-4 py-3 last:border-r-0">
      <div className="font-sans text-xs font-semibold tracking-normal text-tx-3">{label}</div>
      <div className="mt-1 truncate font-sans text-sm font-semibold text-tx-0">{value}</div>
    </div>
  );
}

