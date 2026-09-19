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

export function signalTimeFromTimestamp(
  timestamp: string,
): SignalReferenceTime | undefined {
  const eventTime = Date.parse(timestamp);
  if (!Number.isFinite(eventTime)) return undefined;
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
