/**
 * Structured anomaly-answer parsing + evidence link routing for Mole Intelligence
 * page. The model is asked to return a JSON answer (optionally fenced); we
 * parse it defensively and fall back to plain text when it is not structured.
 */

export interface AnomalyPoint {
  metric?: string;
  stream?: string;
  observed?: string;
  expected?: string;
  timestamp?: string;
  description?: string;
}

export interface EvidenceRef {
  kind?: string;
  label?: string;
  stream?: string;
  time_range?: { start_micros: number; end_micros: number };
  trace_id?: string;
  query?: string;
  object_key?: string;
  href?: string;
  route?: string;
}

export interface RelatedLink {
  label: string;
  href?: string;
  route?: string;
}

export interface StructuredAnswer {
  summary?: string;
  anomaly_points?: AnomalyPoint[];
  evidence?: EvidenceRef[];
  likely_causes?: string[];
  limitations?: string[];
  suggested_next_steps?: string[];
  related_links?: RelatedLink[];
  confidence?: number | 'high' | 'medium' | 'low';
}

const ANSWER_KEYS = [
  'summary',
  'anomaly_points',
  'evidence',
  'likely_causes',
  'limitations',
  'suggested_next_steps',
  'related_links',
];

function hasAnswerShape(o: Record<string, unknown>): boolean {
  return ANSWER_KEYS.some((k) => k in o);
}

/** Extract a structured answer from model output, or null if it is plain text. */
export function parseStructuredAnswer(content: string): StructuredAnswer | null {
  const candidates: string[] = [];
  const fence = content.match(/```(?:json)?\s*([\s\S]*?)```/i);
  if (fence?.[1]) candidates.push(fence[1]);
  candidates.push(content);
  for (const c of candidates) {
    const trimmed = c.trim();
    if (!trimmed.startsWith('{')) continue;
    try {
      const obj = JSON.parse(trimmed) as Record<string, unknown>;
      if (obj && typeof obj === 'object' && hasAnswerShape(obj)) {
        return normalizeStructuredAnswer(obj);
      }
    } catch {
      // not JSON — keep trying / fall through to plain text
    }
  }
  return null;
}

function normalizeStructuredAnswer(value: Record<string, unknown>): StructuredAnswer {
  const answer: StructuredAnswer = {};
  const summary = stringValue(value.summary);
  if (summary) answer.summary = summary;
  const anomalyPoints = recordArray(value.anomaly_points);
  if (anomalyPoints.length > 0) answer.anomaly_points = anomalyPoints as AnomalyPoint[];
  const evidence = recordArray(value.evidence);
  if (evidence.length > 0) answer.evidence = evidence as EvidenceRef[];
  const likelyCauses = stringArray(value.likely_causes);
  if (likelyCauses.length > 0) answer.likely_causes = likelyCauses;
  const limitations = stringArray(value.limitations);
  if (limitations.length > 0) answer.limitations = limitations;
  const nextSteps = stringArray(value.suggested_next_steps);
  if (nextSteps.length > 0) answer.suggested_next_steps = nextSteps;
  const links = recordArray(value.related_links).filter((item) => stringValue(item.label));
  if (links.length > 0) answer.related_links = links as unknown as RelatedLink[];
  const confidence = value.confidence;
  if (
    typeof confidence === 'number' ||
    confidence === 'high' ||
    confidence === 'medium' ||
    confidence === 'low'
  ) {
    answer.confidence = confidence;
  }
  return answer;
}

function recordArray(value: unknown): Array<Record<string, unknown>> {
  if (!Array.isArray(value)) return [];
  return value.filter(
    (item): item is Record<string, unknown> =>
      Boolean(item) && typeof item === 'object' && !Array.isArray(item),
  );
}

function stringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    const normalized = stringValue(item);
    return normalized ? [normalized] : [];
  });
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

/**
 * Build an in-app route for an evidence row, preserving time range and stream
 * context. Returns null for evidence with no navigable target (e.g. an archive
 * object key), which the UI renders as static text.
 */
export function evidenceHref(ev: EvidenceRef): string | null {
  if (ev.href) return ev.href;
  if (ev.route) return ev.route;
  const params = new URLSearchParams();
  if (ev.time_range?.start_micros) params.set('from', String(ev.time_range.start_micros));
  if (ev.time_range?.end_micros) params.set('to', String(ev.time_range.end_micros));
  if (ev.stream) params.set('stream', ev.stream);
  const qs = params.toString() ? `?${params.toString()}` : '';
  switch ((ev.kind ?? '').toLowerCase()) {
    case 'logs':
    case 'log':
      return `/logs${qs}`;
    case 'metrics':
    case 'metric':
      return `/metrics${qs}`;
    case 'traces':
    case 'trace':
      return ev.trace_id ? `/traces/${encodeURIComponent(ev.trace_id)}` : `/traces${qs}`;
    case 'alerts':
    case 'alert':
    case 'incident':
      return '/alerts';
    default:
      return null;
  }
}
