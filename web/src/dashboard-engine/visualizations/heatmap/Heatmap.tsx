import { heatmapIntensity, type HeatmapModel } from './model';
import { heatmapColor } from '../shared/colors';
import { useElementSize } from '../shared/MeasuredContainer';

export function Heatmap({
  model,
  height,
  colorScheme,
}: {
  model: HeatmapModel;
  height: number;
  colorScheme: unknown;
}) {
  const [ref, size] = useElementSize({ width: 480, height });
  const color = heatmapColor(colorScheme);
  const compact = size.width < 260 || size.height < 120;
  const rowHeight = Math.max(8, Math.min(24, (size.height - 24) / model.rows.length));
  const labelWidth = compact ? '4rem' : '7rem';

  return (
    <div
      ref={ref}
      role="img"
      aria-label={`Heatmap with ${model.rows.length} series and ${model.columns} columns; values ${formatNumber(model.min)} to ${formatNumber(model.max)}`}
      className="flex h-full min-h-20 min-w-0 flex-col justify-center overflow-hidden"
    >
      <div className="min-h-0 overflow-y-auto">
        {model.rows.map((row) => (
          <div
            key={row.id}
            className="grid min-w-0 items-center gap-2"
            style={{ gridTemplateColumns: `${labelWidth} minmax(0, 1fr)`, height: rowHeight }}
          >
            <span className="truncate text-right font-sans text-type-micro text-tx-3" title={row.name}>
              {row.name}
            </span>
            <span
              className="grid h-[calc(100%-2px)] min-h-1 gap-px"
              style={{ gridTemplateColumns: `repeat(${model.columns}, minmax(1px, 1fr))` }}
            >
              {row.values.map((value, index) => (
                <span
                  aria-hidden="true"
                  data-testid="heatmap-cell"
                  key={index}
                  title={`${row.name} · ${columnTitle(index, model)}: ${value === null ? 'No value' : formatNumber(value)}`}
                  className="min-w-px rounded-[1px]"
                  style={
                    value === null
                      ? { backgroundColor: 'var(--bd-0)', opacity: 0.35 }
                      : {
                          backgroundColor: color,
                          opacity: heatmapIntensity(value, model),
                        }
                  }
                />
              ))}
            </span>
          </div>
        ))}
      </div>
      <div
        aria-hidden="true"
        className="mt-1 grid gap-2 font-mono text-type-micro text-tx-3"
        style={{ gridTemplateColumns: `${labelWidth} minmax(0, 1fr)` }}
      >
        <span />
        <span className="flex justify-between">
          <span>{model.firstColumnLabel}</span>
          <span>{model.lastColumnLabel}</span>
        </span>
      </div>
    </div>
  );
}

function columnTitle(index: number, model: HeatmapModel): string {
  const start = index * model.windowSize + 1;
  const end = Math.min(model.totalSamples, (index + 1) * model.windowSize);
  return start === end ? String(start) : `${start}–${end}`;
}

function formatNumber(value: number): string {
  return value.toLocaleString(undefined, { maximumFractionDigits: 3 });
}
