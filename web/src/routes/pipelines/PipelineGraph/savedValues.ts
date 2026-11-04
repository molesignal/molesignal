import type { TransformStep } from './model';
import { compileProcessor, type ProcessorConfig } from '../processors/catalog';

/** What a pipeline saved, read back: anything of another shape reads as nothing. */
export function stepObject(value: unknown): Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

export function stringsFrom(value: unknown, fallback?: string | string[]): string[] {
  if (Array.isArray(value)) return value.map(String).map((item) => item.trim()).filter(Boolean);
  if (typeof value === 'string' && value.trim()) return [value.trim()];
  if (Array.isArray(fallback)) return fallback;
  return fallback ? [fallback] : [];
}

/** A whole number from 0 to `max` as saved, or `fallback` for anything else. */
export function wholeNumber(value: unknown, fallback: number, max: number): number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= max ? value : fallback;
}

/** The processor of a step, while its script is still the one the processor generates. */
export function processorOf(step: TransformStep): ProcessorConfig | undefined {
  return step.processor && compileProcessor(step.processor) === step.script
    ? step.processor
    : undefined;
}
