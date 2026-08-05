import {
  formatSegmentBoundary,
  type StateSegment,
  type StateTimelineModel,
} from './model';
import { useElementSize } from '../shared/MeasuredContainer';

export function StateTimeline({
  model,
  height,
  showValues,
}: {
  model: StateTimelineModel;
  height: number;
  showValues: 'auto' | 'always' | 'never';
}) {
  const [ref, size] = useElementSize({ width: 480, height });
  const compact = size.width < 300 || size.height < 130;
  const labelWidth = compact ? 64 : 112;
  const plotWidth = Math.max(1, size.width - labelWidth - 8);
  const showLegend = !compact && model.legend.length > 1;
  const rowHeight = Math.max(
    18,
    Math.min(32, (size.height - (showLegend ? 34 : 16)) / model.rows.length),
  );

  return (
    <div
      ref={ref}
      role="img"
      aria-label={`State timeline with ${model.rows.length} rows and ${model.legend.length}${model.legendTruncated ? ' or more' : ''} states`}
      className="flex h-full min-h-20 min-w-0 flex-col overflow-hidden"
    >
      {showLegend && (
        <div aria-hidden="true" className="flex h-6 shrink-0 items-center gap-3 overflow-hidden pl-2">
          {model.legend.map((item) => (
            <span key={item.id} className="flex min-w-0 items-center gap-1 text-type-micro text-tx-3">
              <span className="h-2 w-2 shrink-0 rounded-[1px]" style={{ backgroundColor: item.color }} />
              <span className="max-w-24 truncate">{item.text}</span>
            </span>
          ))}
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {model.rows.map((row) => (
          <div
            key={row.id}
            className="grid min-w-0 items-center gap-2"
            style={{ gridTemplateColumns: `${labelWidth}px minmax(0, 1fr)`, height: rowHeight }}
          >
            <span className="truncate text-right font-sans text-xs text-tx-3" title={row.name}>
              {row.name}
            </span>
            <span className="relative h-[calc(100%-4px)] min-h-3 overflow-hidden rounded-sm bg-bg-3">
              {row.segments.map((segment) => (
                <TimelineSegment
                  key={segment.id}
                  rowName={row.name}
                  segment={segment}
                  model={model}
                  plotWidth={plotWidth}
                  showValues={showValues}
                />
              ))}
            </span>
          </div>
        ))}
      </div>
      <div
        aria-hidden="true"
        className="grid shrink-0 gap-2 font-mono text-type-micro text-tx-3"
        style={{ gridTemplateColumns: `${labelWidth}px minmax(0, 1fr)` }}
      >
        <span />
        <span className="flex justify-between">
          <span>{model.startLabel}</span>
          <span>{model.endLabel}</span>
        </span>
      </div>
    </div>
  );
}

function TimelineSegment({
  rowName,
  segment,
  model,
  plotWidth,
  showValues,
}: {
  rowName: string;
  segment: StateSegment;
  model: StateTimelineModel;
  plotWidth: number;
  showValues: 'auto' | 'always' | 'never';
}) {
  const span = model.end - model.start;
  const left = ((segment.start - model.start) / span) * 100;
  const width = ((segment.end - segment.start) / span) * 100;
  const widthPx = (width / 100) * plotWidth;
  const visible =
    showValues === 'always' ||
    (showValues === 'auto' && widthPx >= segment.text.length * 6 + 12);
  return (
    <span
      aria-hidden="true"
      data-testid="state-timeline-segment"
      title={`${rowName} · ${segment.text}: ${formatSegmentBoundary(segment.start, model)}–${formatSegmentBoundary(segment.end, model)}`}
      className="absolute inset-y-0 min-w-px overflow-hidden border-r border-bg-1 px-1 text-center font-sans text-type-micro leading-5 text-bg-0"
      style={{ left: `${left}%`, width: `${width}%`, backgroundColor: segment.color }}
    >
      {visible && (
        <span data-testid="state-timeline-label" className="block truncate">
          {segment.text}
        </span>
      )}
    </span>
  );
}
