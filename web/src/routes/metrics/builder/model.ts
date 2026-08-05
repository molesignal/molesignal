import type { PromqlCapabilityItem } from '@/api/query';

export type BuilderTransform = string;
export type BuilderAggregation = 'none' | 'sum' | 'avg' | 'max' | 'min';
export type BuilderMatcherOperator = '=' | '!=' | '=~' | '!~';
export type BuilderFunctionInput = 'vector' | 'range';

export interface BuilderFunctionOption {
  name: string;
  detail: string;
  documentation: string;
  input: BuilderFunctionInput | null;
}

export interface BuilderMatcher {
  id: string;
  name: string;
  operator: BuilderMatcherOperator;
  value: string;
}

export interface BuilderQuery {
  metric: string;
  transform: BuilderTransform;
  aggregation: BuilderAggregation;
  range: string;
  matchers: BuilderMatcher[];
}

const FALLBACK_FUNCTIONS: PromqlCapabilityItem[] = [
  {
    label: 'rate',
    insert_text: 'rate(${1:metric}[${2:5m}])',
    detail: 'rate(range-vector)',
    documentation: 'Per-second average rate over a range vector.',
    kind: 'function',
  },
  {
    label: 'irate',
    insert_text: 'irate(${1:metric}[${2:5m}])',
    detail: 'irate(range-vector)',
    documentation: 'Instantaneous per-second rate from the last two samples.',
    kind: 'function',
  },
  {
    label: 'increase',
    insert_text: 'increase(${1:metric}[${2:5m}])',
    detail: 'increase(range-vector)',
    documentation: 'Total increase over a range vector.',
    kind: 'function',
  },
];

export const BUILDER_AGGREGATIONS: BuilderAggregation[] = [
  'none',
  'sum',
  'avg',
  'max',
  'min',
];

export const BUILDER_RANGES = ['1m', '5m', '10m', '15m', '30m', '1h', '6h', '1d'];

export function builderFunctionOptions(
  capabilities: PromqlCapabilityItem[] | undefined,
): BuilderFunctionOption[] {
  const source = capabilities?.length ? capabilities : FALLBACK_FUNCTIONS;
  const byName = new Map<string, BuilderFunctionOption>();
  byName.set('none', {
    name: 'none',
    detail: '',
    documentation: '',
    input: 'vector',
  });
  for (const item of source) {
    if (!item.label || byName.has(item.label)) continue;
    byName.set(item.label, {
      name: item.label,
      detail: item.detail,
      documentation: item.documentation,
      input: builderFunctionInput(item),
    });
  }
  return [...byName.values()];
}

function builderFunctionInput(
  item: PromqlCapabilityItem,
): BuilderFunctionInput | null {
  const signature = item.detail.replace(/\s+/g, '');
  if (signature === `${item.label}(range-vector)`) return 'range';
  if (
    signature === `${item.label}(vector)` ||
    signature === `${item.label}(vector?)`
  ) {
    return 'vector';
  }
  return null;
}

export function emptyBuilderQuery(): BuilderQuery {
  return {
    metric: '',
    transform: 'none',
    aggregation: 'none',
    range: '5m',
    matchers: [],
  };
}

export function newBuilderMatcher(index: number): BuilderMatcher {
  return {
    id: `matcher-${index}`,
    name: '',
    operator: '=',
    value: '',
  };
}

export function composeBuilderPromql(
  query: BuilderQuery,
  functions = builderFunctionOptions(undefined),
): string {
  if (!query.metric) return '';

  const completeMatchers = query.matchers.filter(
    (matcher) => matcher.name && matcher.value,
  );
  const matcherExpression = completeMatchers
    .map(
      (matcher) =>
        `${matcher.name}${matcher.operator}"${escapeMatcherValue(matcher.value)}"`,
    )
    .join(',');
  const selector = matcherExpression
    ? `${query.metric}{${matcherExpression}}`
    : query.metric;
  const selectedFunction = functions.find(
    (option) => option.name === query.transform,
  );
  const transformed = query.transform === 'none'
    ? selector
    : selectedFunction?.input === 'vector'
      ? `${query.transform}(${selector})`
      : `${query.transform}(${selector}[${query.range || '5m'}])`;

  return query.aggregation === 'none'
    ? transformed
    : `${query.aggregation}(${transformed})`;
}

export function parseBuilderPromql(
  expression: string,
  functions = builderFunctionOptions(undefined),
): BuilderQuery | null {
  const trimmed = expression.trim();
  if (!trimmed) return emptyBuilderQuery();

  let inner = trimmed;
  let aggregation: BuilderAggregation = 'none';
  const aggregationMatch = inner.match(/^(sum|avg|max|min)\((.*)\)$/s);
  if (aggregationMatch) {
    aggregation = aggregationMatch[1] as BuilderAggregation;
    inner = aggregationMatch[2]!.trim();
  }

  let transform: BuilderTransform = 'none';
  let range = '5m';
  for (const option of functions) {
    if (option.name === 'none' || option.input === null) continue;
    const escapedName = option.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const match = option.input === 'range'
      ? inner.match(new RegExp(`^${escapedName}\\((.*)\\[([^\\]\\s]+)\\]\\)$`, 's'))
      : inner.match(new RegExp(`^${escapedName}\\((.*)\\)$`, 's'));
    if (!match) continue;
    transform = option.name;
    inner = match[1]!.trim();
    if (option.input === 'range') range = match[2]!;
    break;
  }

  const selectorMatch = inner.match(
    /^([a-zA-Z_:][a-zA-Z0-9_:]*)(?:\{(.*)\})?$/s,
  );
  if (!selectorMatch) return null;

  const matchers = parseMatchers(selectorMatch[2] ?? '');
  if (matchers === null) return null;

  return {
    metric: selectorMatch[1]!,
    transform,
    aggregation,
    range,
    matchers,
  };
}

function parseMatchers(source: string): BuilderMatcher[] | null {
  if (!source.trim()) return [];

  const matchers: BuilderMatcher[] = [];
  const pattern = /\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*(=~|!~|!=|=)\s*"((?:\\.|[^"\\])*)"\s*(?:,|$)/gy;
  let offset = 0;

  while (offset < source.length) {
    pattern.lastIndex = offset;
    const match = pattern.exec(source);
    if (!match || match.index !== offset) return null;
    matchers.push({
      id: `matcher-${matchers.length}`,
      name: match[1]!,
      operator: match[2] as BuilderMatcherOperator,
      value: unescapeMatcherValue(match[3]!),
    });
    offset = pattern.lastIndex;
  }

  return matchers;
}

function escapeMatcherValue(value: string): string {
  return value
    .replace(/\\/g, '\\\\')
    .replace(/"/g, '\\"')
    .replace(/\n/g, '\\n')
    .replace(/\r/g, '\\r')
    .replace(/\t/g, '\\t');
}

function unescapeMatcherValue(value: string): string {
  return value.replace(/\\([\\"nrt])/g, (_match, escaped: string) => {
    if (escaped === 'n') return '\n';
    if (escaped === 'r') return '\r';
    if (escaped === 't') return '\t';
    return escaped;
  });
}
