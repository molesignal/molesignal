import type * as Monaco from 'monaco-editor/editor/editor.api.js';

/**
 * How the editor colours a field query. A name is a field by where it stands,
 * the first argument of MATCH or the start of a comparison, never by the name
 * itself: the editor knows no stream, and a list of its own would colour names
 * a stream lacks and leave its real ones plain. Comparisons, conjunctions and
 * functions read in any case, as the query does.
 */
export const FIELD_QUERY_TOKENS: Monaco.languages.IMonarchLanguage = {
  ignoreCase: true,
  operators: ['AND', 'OR', 'contains', 'like', 'eq', 'ne'],
  tokenizer: {
    root: [
      [/(MATCH_TEXT|MATCH)(\s*\(\s*)([a-zA-Z_][\w.]*)/, ['function', 'delimiter', 'field.query']],
      // A conjunction before a bracket opens a group, not a call.
      [/(?:AND|OR)(?=\s*\()/, 'operator.query'],
      [/[a-zA-Z_][\w.]*\s*(?=\()/, 'function'],
      [/[a-zA-Z_][\w.]*(?=\s*(?:!=|>=|<=|=|>|<)|\s+(?:contains|like|eq|ne)\b)/, 'field.query'],
      [/[a-zA-Z_][\w.]*/, { cases: { '@operators': 'operator.query', '@default': 'identifier' } }],
      [/"([^"\\]|\\.)*$/, 'string.invalid'],
      [/"/, 'string.query', '@string_double'],
      [/'([^'\\]|\\.)*$/, 'string.invalid'],
      [/'/, 'string.query', '@string_single'],
      [/\b-?\d+(?:\.\d+)?(?:ms|s|m|h|d|w)?\b/, 'number'],
      [/[()[\],]/, 'delimiter'],
      [/!=|>=|<=|=|>|</, 'operator.query'],
    ],
    string_double: [
      [/[^\\"]+/, 'string.query'],
      [/\\./, 'string.escape'],
      [/"/, 'string.query', '@pop'],
    ],
    string_single: [
      [/[^\\']+/, 'string.query'],
      [/\\./, 'string.escape'],
      [/'/, 'string.query', '@pop'],
    ],
  },
};
