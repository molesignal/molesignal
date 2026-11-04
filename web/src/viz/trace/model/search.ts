import { attributeText } from './span';
import type { TraceNode, TraceTree } from './tree';

/**
 * One condition of a span search: words looked for anywhere in a span, a tag
 * that has to hold a value (`key:value` or `key=value`), or a duration
 * compared (`duration>100ms`).
 */
type Condition =
  | { kind: 'text'; text: string }
  | { kind: 'tag'; key: string; value: string }
  | { kind: 'duration'; op: '>' | '>=' | '<' | '<='; ns: number };

const UNITS: Record<string, number> = { ns: 1, us: 1_000, µs: 1_000, μs: 1_000, ms: 1_000_000, s: 1_000_000_000 };

/** Reads a search. Words in quotes stay together; everything else splits on spaces. */
export function parseSpanSearch(query: string): Condition[] {
  const tokens = query.match(/"[^"]*"|\S+/g) ?? [];

  return tokens.flatMap((raw): Condition[] => {
    const token = raw.replace(/^"|"$/g, '').trim();
    if (!token) return [];

    const duration = /^duration\s*(>=|<=|>|<)\s*(\d+(?:\.\d+)?)\s*(ns|us|µs|μs|ms|s)?$/i.exec(token);
    if (duration) {
      const unit = UNITS[(duration[3] ?? 'ms').toLowerCase()] ?? UNITS.ms!;
      return [{ kind: 'duration', op: duration[1] as '>' | '>=' | '<' | '<=', ns: Number(duration[2]) * unit }];
    }

    const tag = raw.startsWith('"') ? null : /^([^:=\s]+)\s*[:=]\s*(.+)$/.exec(token);
    if (tag) return [{ kind: 'tag', key: tag[1]!.toLowerCase(), value: tag[2]!.replace(/^"|"$/g, '').toLowerCase() }];

    return [{ kind: 'text', text: token.toLowerCase() }];
  });
}

/** The spans a search finds: every one of its conditions holds. An empty search finds none. */
export function searchSpans(tree: TraceTree, query: string): Set<string> {
  const conditions = parseSpanSearch(query);
  const found = new Set<string>();
  if (conditions.length === 0) return found;

  for (const node of tree.nodes) {
    if (conditions.every((condition) => holds(node, condition))) found.add(node.id);
  }

  return found;
}

function holds(node: TraceNode, condition: Condition): boolean {
  switch (condition.kind) {
    case 'duration':
      return compare(node.durationNs, condition.op, condition.ns);
    case 'tag':
      return tagValues(node, condition.key).some((value) => value.includes(condition.value));
    case 'text':
      return searchableText(node).includes(condition.text);
  }
}

function compare(value: number, op: '>' | '>=' | '<' | '<=', against: number): boolean {
  if (op === '>') return value > against;
  if (op === '>=') return value >= against;
  if (op === '<') return value < against;
  return value <= against;
}

/** The values a span has under a name: its own facts first, then its tags and its resource's. */
function tagValues(node: TraceNode, key: string): string[] {
  const { span } = node;
  const facts: Record<string, string> = {
    service: span.service,
    'service.name': span.service,
    operation: span.operation,
    name: span.operation,
    status: span.status,
    status_code: span.status,
    span_id: span.spanId,
    parent_span_id: span.parentSpanId ?? '',
  };

  const values = key in facts ? [facts[key]!] : [];

  for (const attributes of [span.attributes, span.resource?.attributes]) {
    for (const [name, value] of Object.entries(attributes ?? {})) {
      if (name.toLowerCase() === key) values.push(attributeText(value));
    }
  }

  return values.map((value) => value.toLowerCase());
}

const textCache = new WeakMap<TraceNode, string>();

/** Everything a span says, as one lowercase text, read once per span. */
function searchableText(node: TraceNode): string {
  const cached = textCache.get(node);
  if (cached !== undefined) return cached;

  const { span } = node;
  const parts = [span.service, span.operation, span.spanId, span.status, span.statusMessage ?? ''];

  for (const attributes of [span.attributes, span.resource?.attributes]) {
    for (const value of Object.values(attributes ?? {})) parts.push(attributeText(value));
  }

  const text = parts.join('\n').toLowerCase();
  textCache.set(node, text);
  return text;
}
