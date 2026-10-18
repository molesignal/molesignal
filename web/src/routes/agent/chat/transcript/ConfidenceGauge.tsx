import type { TFunction } from 'i18next';
import * as React from 'react';

import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

export type ConfidenceLevel = 'high' | 'medium' | 'low';

/**
 * How full the arc is at each level, and the two ends of its colour: the higher
 * the confidence, the further the arc reaches and the greener it ends.
 */
const LEVELS: Record<ConfidenceLevel, { reach: number; from: string; to: string }> = {
  high: { reach: 0.85, from: 'var(--green-soft)', to: 'var(--green)' },
  medium: { reach: 0.55, from: 'var(--yellow-soft)', to: 'var(--yellow)' },
  low: { reach: 0.25, from: 'var(--orange-soft)', to: 'var(--red)' },
};

/** A level from what the answer says: a word, or a number from 0 to 1. */
export function confidenceLevel(value: number | ConfidenceLevel): ConfidenceLevel {
  if (typeof value === 'string') return value;
  if (value >= 0.75) return 'high';
  if (value >= 0.45) return 'medium';
  return 'low';
}

const CENTER = 12;
const RADIUS = 8.5;
/** The arc opens at the bottom: it runs a quarter short of a circle, from lower left to lower right. */
const START = 135;
const SWEEP = 270;

function point(degrees: number) {
  const radians = (degrees * Math.PI) / 180;
  return {
    x: CENTER + RADIUS * Math.cos(radians),
    y: CENTER + RADIUS * Math.sin(radians),
  };
}

function arc(reach: number): { d: string; end: { x: number; y: number } } {
  const sweep = SWEEP * reach;
  const start = point(START);
  const end = point(START + sweep);
  const large = sweep > 180 ? 1 : 0;
  return {
    d: `M ${start.x.toFixed(2)} ${start.y.toFixed(2)} A ${RADIUS} ${RADIUS} 0 ${large} 1 ${end.x.toFixed(2)} ${end.y.toFixed(2)}`,
    end,
  };
}

/**
 * How sure the answer is, as a gauge: an open arc filled to the level, in a
 * colour that runs from light to deep along it. Its name is in the tooltip.
 */
export function ConfidenceGauge({
  value,
  t,
}: {
  value: number | ConfidenceLevel;
  t: TFunction<'agent'>;
}) {
  // An id a `url(#…)` reference reads in every browser: letters, digits and dashes.
  const gradientId = `confidence-${React.useId().replace(/[^a-zA-Z0-9-]/g, '')}`;
  const level = confidenceLevel(value);
  const { reach, from, to } = LEVELS[level];
  // A number says how far the arc reaches; a word reaches as far as its level.
  const filled = typeof value === 'number' ? Math.min(1, Math.max(0.08, value)) : reach;
  const track = arc(1);
  const progress = arc(filled);
  const start = point(START);
  const label = `${t('answer.confidence')} ${t(`answer.confidence_levels.${level}`)}`;

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          role="img"
          aria-label={label}
          data-agent-confidence={level}
          className="grid h-7 w-7 shrink-0 place-items-center"
        >
          <svg viewBox="0 0 24 24" className="h-4 w-4" aria-hidden="true">
            <defs>
              <linearGradient
                id={gradientId}
                gradientUnits="userSpaceOnUse"
                x1={start.x}
                y1={start.y}
                x2={progress.end.x}
                y2={progress.end.y}
              >
                <stop offset="0" style={{ stopColor: from }} />
                <stop offset="1" style={{ stopColor: to }} />
              </linearGradient>
            </defs>
            <path
              d={track.d}
              fill="none"
              strokeWidth={2.75}
              strokeLinecap="round"
              className="stroke-bd-2"
            />
            <path
              d={progress.d}
              fill="none"
              stroke={`url(#${gradientId})`}
              strokeWidth={2.75}
              strokeLinecap="round"
            />
          </svg>
        </span>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
