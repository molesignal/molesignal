export interface InvestigationEvidenceItem {
  toolCallId: string;
  tool: string;
  status: 'success' | 'error' | 'running';
  summary: string;
  arguments?: unknown;
  rowCount?: number;
  scannedRows?: number;
  tookMs?: number;
  objectKey?: string;
}

export function parseInvestigationEvidence(value: unknown): InvestigationEvidenceItem[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((entry, index) => {
    if (!isRecord(entry)) return [];
    const tool = stringValue(entry.tool);
    if (!tool) return [];
    const rawStatus = stringValue(entry.status);
    const status: InvestigationEvidenceItem['status'] =
      rawStatus === 'error' ? 'error' : rawStatus === 'running' ? 'running' : 'success';
    const item: InvestigationEvidenceItem = {
      toolCallId: stringValue(entry.tool_call_id) || `${tool}-${index}`,
      tool,
      status,
      summary: stringValue(entry.summary),
    };
    if ('arguments' in entry) item.arguments = entry.arguments;
    const rowCount = numberValue(entry.row_count);
    const scannedRows = numberValue(entry.scanned_rows);
    const tookMs = numberValue(entry.took_ms);
    const objectKey = stringValue(entry.object_key);
    if (rowCount !== undefined) item.rowCount = rowCount;
    if (scannedRows !== undefined) item.scannedRows = scannedRows;
    if (tookMs !== undefined) item.tookMs = tookMs;
    if (objectKey) item.objectKey = objectKey;
    return [item];
  });
}

export function fallbackToolLabel(name: string): string {
  return name
    .replace(/^(get|list|query|search|propose)_/, '')
    .split('_')
    .filter(Boolean)
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(' ');
}

export function formatInvestigationDuration(durationMs: number): string {
  if (!Number.isFinite(durationMs) || durationMs < 0) return '';
  const totalSeconds = Math.floor(durationMs / 1000);
  if (totalSeconds < 1) return '<1s';
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return [
    hours > 0 ? `${hours}h` : '',
    minutes > 0 ? `${minutes}m` : '',
    seconds > 0 ? `${seconds}s` : '',
  ]
    .filter(Boolean)
    .join(' ');
}

export function isRedundantInvestigationSummary(
  summary: string,
  rowCount?: number,
): boolean {
  if (rowCount === undefined) return false;
  const normalized = summary.trim().toLowerCase().replace(/\s+/g, ' ');
  return [
    `${rowCount} row`,
    `${rowCount} rows`,
    `${rowCount} 条`,
    `返回 ${rowCount} 条`,
  ].includes(normalized);
}

/**
 * Some providers can leak their internal DSML tool-call transport into the
 * assistant text stream. The real tool calls are already rendered from
 * evidence, so exposing the protocol body duplicates data and reveals
 * implementation details. Remove complete and in-flight DSML tool blocks
 * while preserving the surrounding product answer.
 */
export function sanitizeAssistantContent(content: string): string {
  return content
    .replace(
      /<[|｜]+DSML[|｜]+tool_calls\s*>[\s\S]*?(?:<\/[|｜]+DSML[|｜]+tool_calls\s*>|$)/gi,
      '',
    )
    .replace(
      /<[|｜]+DSML[|｜]+invoke\b[^>]*>[\s\S]*?(?:<\/[|｜]+DSML[|｜]+invoke\s*>|$)/gi,
      '',
    )
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

function numberValue(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}
