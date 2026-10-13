import {
  detectSignalTypeForLabel,
  type SignalReferenceTime,
  type SignalReferenceType,
} from '@/shell/SignalReference';

export interface RelatedSignalField {
  field: string;
  type: SignalReferenceType;
  value: string;
}

export function stringLabelsFromRecord(
  record: Record<string, unknown>,
): Record<string, string> {
  const labels: Record<string, string> = {};
  visitRecord(record, (path, value) => {
    const stringValue = String(value);
    if (!stringValue) return;
    labels[path] = stringValue;
    const leaf = leafFieldName(path);
    if (!(leaf in labels)) labels[leaf] = stringValue;
  });
  return labels;
}

export function relatedSignalsFromRecord(
  record: Record<string, unknown>,
): RelatedSignalField[] {
  const signals: RelatedSignalField[] = [];
  const seen = new Set<string>();
  visitRecord(record, (path, value) => {
    const type =
      detectSignalTypeForLabel(path) ??
      detectSignalTypeForLabel(leafFieldName(path));
    if (!type) return;
    const displayValue = String(value);
    const key = `${type}:${displayValue}`;
    if (seen.has(key)) return;
    seen.add(key);
    signals.push({ field: path, type, value: displayValue });
  });
  return signals.slice(0, 6);
}

/**
 * The stretch of time a signal named by a log is opened in: five minutes on
 * either side of the log. It is taken from the instant itself, in
 * microseconds: the moment as it is written for the reader is in their zone
 * and their format, and cannot be read back.
 */
export function signalTimeAround(micros: number | null): SignalReferenceTime | undefined {
  if (micros == null || !Number.isFinite(micros)) return undefined;

  const eventTime = micros / 1_000;

  return {
    from: new Date(eventTime - 5 * 60_000).toISOString(),
    to: new Date(eventTime + 5 * 60_000).toISOString(),
  };
}

function visitRecord(
  record: Record<string, unknown>,
  visit: (path: string, value: string | number | boolean) => void,
): void {
  const walk = (value: unknown, path: string) => {
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      for (const [key, nested] of Object.entries(value as Record<string, unknown>)) {
        walk(nested, path ? `${path}.${key}` : key);
      }
      return;
    }
    if (
      typeof value === 'string' ||
      typeof value === 'number' ||
      typeof value === 'boolean'
    ) {
      visit(path, value);
    }
  };
  walk(record, '');
}

function leafFieldName(field: string): string {
  const parts = field
    .toLowerCase()
    .replace(/\[(\d+)\]/g, '.$1')
    .split('.')
    .filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1]! : field.toLowerCase();
}
