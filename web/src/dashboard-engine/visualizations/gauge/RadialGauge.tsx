import {
  drawRadialArcPath,
  GAUGE_START_ANGLE,
  GAUGE_SWEEP_ANGLE,
  gaugeValueRatio,
  pointOnRadialArc,
  type GaugeRange,
  type GaugeThresholdInterval,
} from './geometry';

const VIEWBOX_WIDTH = 260;
const VIEWBOX_HEIGHT = 176;
const CENTER_X = VIEWBOX_WIDTH / 2;
const CENTER_Y = 112;
const GAUGE_RADIUS = 80;
const THRESHOLD_RADIUS = 94;
const THRESHOLD_LABEL_RADIUS = 108;
const COMPACT_HEIGHT = 132;

export interface RadialGaugeProps {
  value: number;
  valueText: string;
  name: string;
  range: GaugeRange;
  minimumText: string;
  maximumText: string;
  color?: string | undefined;
  thresholdIntervals: readonly GaugeThresholdInterval[];
  showThresholdMarkers: boolean;
  showThresholdLabels: boolean;
  height: number;
}

export function RadialGauge({
  value,
  valueText,
  name,
  range,
  minimumText,
  maximumText,
  color,
  thresholdIntervals,
  showThresholdMarkers,
  showThresholdLabels,
  height,
}: RadialGaugeProps) {
  const compact = height < COMPACT_HEIGHT;
  const ratio = gaugeValueRatio(value, range);
  const activePath = drawRadialArcPath(
    GAUGE_START_ANGLE,
    ratio * GAUGE_SWEEP_ANGLE,
    GAUGE_RADIUS,
    CENTER_X,
    CENTER_Y,
  );
  const ariaLabel = `${name}: ${valueText}; ${minimumText}–${maximumText}`;

  return (
    <div className="grid h-full min-h-0 w-full place-items-center overflow-hidden">
      <svg
        role="img"
        aria-label={ariaLabel}
        focusable="false"
        viewBox={`0 0 ${VIEWBOX_WIDTH} ${VIEWBOX_HEIGHT}`}
        preserveAspectRatio="xMidYMid meet"
        width="100%"
        height="100%"
        className="block max-h-full max-w-full"
      >
        <g aria-hidden="true">
          {showThresholdMarkers &&
            thresholdIntervals.map((interval) => (
              <path
                key={`${interval.start}:${interval.end}:${interval.color}`}
                data-testid="gauge-threshold-interval"
                d={thresholdPath(interval, range)}
                fill="none"
                stroke={interval.color}
                strokeWidth={5}
                strokeLinecap="butt"
              />
            ))}

          <path
            data-testid="gauge-track"
            d={drawRadialArcPath(
              GAUGE_START_ANGLE,
              GAUGE_SWEEP_ANGLE,
              GAUGE_RADIUS,
              CENTER_X,
              CENTER_Y,
            )}
            fill="none"
            stroke="var(--bg-3)"
            strokeWidth={16}
            strokeLinecap="round"
          />

          {activePath && (
            <path
              data-testid="gauge-active-arc"
              d={activePath}
              fill="none"
              stroke={color ?? 'var(--accent)'}
              strokeWidth={16}
              strokeLinecap="round"
            />
          )}

          <text
            x={CENTER_X}
            y={compact ? 108 : 96}
            textAnchor="middle"
            dominantBaseline="middle"
            fontSize={compact ? 25 : 29}
            fontWeight={600}
            className="fill-tx-0 font-mono tabular-nums"
            style={color ? { fill: color } : undefined}
          >
            {truncateLabel(valueText, 18)}
          </text>

          {!compact && (
            <>
              <text
                x={CENTER_X}
                y={122}
                textAnchor="middle"
                dominantBaseline="middle"
                fontSize={11}
                className="fill-tx-3 font-sans"
              >
                {truncateLabel(name, 34)}
              </text>
              <text
                x={45}
                y={161}
                textAnchor="start"
                fontSize={9}
                className="fill-tx-3 font-mono tabular-nums"
              >
                {truncateLabel(minimumText, 14)}
              </text>
              <text
                x={215}
                y={161}
                textAnchor="end"
                fontSize={9}
                className="fill-tx-3 font-mono tabular-nums"
              >
                {truncateLabel(maximumText, 14)}
              </text>
            </>
          )}

          {!compact &&
            showThresholdLabels &&
            thresholdIntervals.slice(1).map((interval) => {
              const ratioAtBoundary = gaugeValueRatio(interval.start, range);
              const point = pointOnRadialArc(
                GAUGE_START_ANGLE + ratioAtBoundary * GAUGE_SWEEP_ANGLE,
                THRESHOLD_LABEL_RADIUS,
                CENTER_X,
                CENTER_Y,
              );
              return (
                <text
                  key={`label:${interval.start}`}
                  data-testid="gauge-threshold-label"
                  x={point.x}
                  y={point.y}
                  textAnchor="middle"
                  dominantBaseline="middle"
                  fontSize={8}
                  className="fill-tx-2 font-mono tabular-nums"
                >
                  {truncateLabel(interval.label ?? String(interval.start), 10)}
                </text>
              );
            })}
        </g>
      </svg>
    </div>
  );
}

function thresholdPath(
  interval: GaugeThresholdInterval,
  range: GaugeRange,
): string {
  const startRatio = gaugeValueRatio(interval.start, range);
  const endRatio = gaugeValueRatio(interval.end, range);
  return drawRadialArcPath(
    GAUGE_START_ANGLE + startRatio * GAUGE_SWEEP_ANGLE,
    (endRatio - startRatio) * GAUGE_SWEEP_ANGLE,
    THRESHOLD_RADIUS,
    CENTER_X,
    CENTER_Y,
  );
}

function truncateLabel(value: string, maximumLength: number): string {
  if (value.length <= maximumLength) return value;
  return `${value.slice(0, maximumLength - 1)}…`;
}
