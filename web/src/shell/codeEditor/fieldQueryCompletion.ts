import type { CodeCompletionItem } from './types';

type Quote = "'" | '"';

export type FieldQueryCompletionContext =
  | { kind: 'expression' }
  | { kind: 'field' }
  | { kind: 'value'; field: string; quote: Quote | null };

export interface FieldQueryCompletionPresentation {
  label: string;
  insertText: string;
  /** Advance from a function's field placeholder to its value placeholder. */
  advanceSnippet: boolean;
}

export function resolveFieldQueryCompletionContext(
  prefix: string,
): FieldQueryCompletionContext {
  const functionValue = prefix.match(
    /(?:^|\bAND\b|\bOR\b)\s*(?:MATCH_TEXT|MATCH)\s*\(\s*(?:"([^"]+)"|([a-zA-Z_][\w.]*))\s*,([\s\S]*)$/i,
  );
  if (functionValue) {
    const field = functionValue[1] ?? functionValue[2];
    const valuePrefix = functionValue[3]?.trimStart() ?? '';
    const quoteState = unclosedQuote(valuePrefix);
    if (
      field
      && quoteState !== 'closed'
      && (quoteState !== null || !/[),]/.test(valuePrefix))
    ) {
      return { kind: 'value', field, quote: quoteState };
    }
  }

  if (
    /(?:^|\bAND\b|\bOR\b)\s*(?:MATCH_TEXT|MATCH)\s*\(\s*(?:"[^"]*|[a-zA-Z_][\w.]*)?$/i
      .test(prefix)
  ) {
    return { kind: 'field' };
  }

  const comparisonValue = prefix.match(
    /(?:^|\bAND\b|\bOR\b|\()\s*(?:"([^"]+)"|([a-zA-Z_][\w.]*))\s*(?:!=|>=|<=|=|>|<|\bcontains\b)\s*([\s\S]*)$/i,
  );
  if (comparisonValue) {
    const field = comparisonValue[1] ?? comparisonValue[2];
    const valuePrefix = comparisonValue[3]?.trimStart() ?? '';
    const quoteState = unclosedQuote(valuePrefix);
    if (field && quoteState !== 'closed') {
      return { kind: 'value', field, quote: quoteState };
    }
  }

  return { kind: 'expression' };
}

export function filterFieldQueryCompletionItems(
  items: ReadonlyArray<CodeCompletionItem>,
  context: FieldQueryCompletionContext,
): CodeCompletionItem[] {
  if (context.kind === 'field') {
    return items.filter((item) => item.kind === 'field');
  }
  if (context.kind === 'value') {
    return items.filter((item) => (
      item.kind === 'value'
      && item.field?.toLowerCase() === context.field.toLowerCase()
    ));
  }
  return items.filter((item) => item.kind !== 'value');
}

export function presentFieldQueryCompletion(
  item: CodeCompletionItem,
  context: FieldQueryCompletionContext,
): FieldQueryCompletionPresentation {
  if (context.kind === 'value' && context.quote && item.value !== undefined) {
    return {
      label: item.value,
      insertText: escapeInsideQuote(item.value, context.quote),
      advanceSnippet: false,
    };
  }
  return {
    label: item.label,
    insertText: item.insertText ?? item.label,
    advanceSnippet: context.kind === 'field' && item.kind === 'field',
  };
}

function unclosedQuote(valuePrefix: string): Quote | null | 'closed' {
  const quote = valuePrefix[0];
  if (quote !== "'" && quote !== '"') return null;
  let escaped = false;
  for (let index = 1; index < valuePrefix.length; index += 1) {
    const character = valuePrefix[index]!;
    if (escaped) {
      escaped = false;
      continue;
    }
    if (character === '\\') {
      escaped = true;
      continue;
    }
    if (character === quote) return 'closed';
  }
  return quote;
}

function escapeInsideQuote(value: string, quote: Quote): string {
  return value
    .replace(/\\/g, '\\\\')
    .replace(new RegExp(quote, 'g'), `\\${quote}`)
    .replace(/\n/g, '\\n')
    .replace(/\r/g, '\\r')
    .replace(/\t/g, '\\t');
}
