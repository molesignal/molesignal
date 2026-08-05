/*
 * Radial arc geometry adapted from Grafana UI v13.1.0:
 * packages/grafana-ui/src/components/RadialGauge/utils.ts
 *
 * Copyright 2015 Grafana Labs
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *   http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 * Modified for MoleSignal: local range and threshold contracts, stable
 * equal-range handling, and deterministic SVG output.
 */

import {
  normalizeValueRange,
  valueRatio,
  type ValueRange,
} from '../shared/range';
import {
  buildThresholdIntervals,
  resolveThresholdColor,
  type ThresholdInterval,
} from '../shared/thresholds';

export const GAUGE_START_ANGLE = 250;
export const GAUGE_SWEEP_ANGLE = 220;

const MAX_ARC_ANGLE = 359.99;
const SVG_DECIMALS = 2;

export type GaugeRange = ValueRange;
export type GaugeThresholdInterval = ThresholdInterval;
export const normalizeGaugeRange = normalizeValueRange;
export const gaugeValueRatio = valueRatio;
export { buildThresholdIntervals, resolveThresholdColor };

export function drawRadialArcPath(
  startAngle: number,
  sweepAngle: number,
  radius: number,
  centerX = 0,
  centerY = 0,
): string {
  if (
    !Number.isFinite(startAngle) ||
    !Number.isFinite(sweepAngle) ||
    !Number.isFinite(radius) ||
    radius <= 0 ||
    sweepAngle <= 0
  ) {
    return '';
  }

  const sweep = Math.min(sweepAngle, MAX_ARC_ANGLE);
  const start = pointOnRadialArc(startAngle, radius, centerX, centerY);
  const end = pointOnRadialArc(startAngle + sweep, radius, centerX, centerY);
  const largeArc = sweep > 180 ? 1 : 0;

  return `M ${start.x} ${start.y} A ${round(radius)} ${round(radius)} 0 ${largeArc} 1 ${end.x} ${end.y}`;
}

export function pointOnRadialArc(
  angle: number,
  radius: number,
  centerX = 0,
  centerY = 0,
): { x: number; y: number } {
  const radians = ((angle - 90) * Math.PI) / 180;
  return {
    x: round(centerX + radius * Math.cos(radians)),
    y: round(centerY + radius * Math.sin(radians)),
  };
}

function round(value: number): number {
  const rounded = Number(value.toFixed(SVG_DECIMALS));
  return Object.is(rounded, -0) ? 0 : rounded;
}
