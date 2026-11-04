import * as monaco from 'monaco-editor/editor/editor.api.js';
import { beforeAll, describe, expect, it } from 'vitest';

import { FIELD_QUERY_TOKENS } from './fieldQueryTokens';

const LANGUAGE = 'field-query';

/** The words of a one-line query the editor draws as `kind`, such as `field.query`. */
function wordsDrawnAs(query: string, kind: string): string[] {
  const [tokens = []] = monaco.editor.tokenize(query, LANGUAGE);

  return tokens
    .map((token, index) => ({
      text: query.slice(token.offset, tokens[index + 1]?.offset ?? query.length).trim(),
      type: token.type,
    }))
    .filter((token) => token.text && token.type.startsWith(`${kind}.`))
    .map((token) => token.text);
}

describe('Field query colours', () => {
  beforeAll(() => {
    monaco.languages.register({ id: LANGUAGE });
    monaco.languages.setMonarchTokensProvider(LANGUAGE, FIELD_QUERY_TOKENS);
  });

  it('draw the name each comparison starts with as a field, whatever the name', () => {
    const query = 'k8s.pod.name = "web-1" AND http.status_code >= 500 AND service.name contains "api"';

    expect(wordsDrawnAs(query, 'field.query')).toEqual(['k8s.pod.name', 'http.status_code', 'service.name']);
  });

  it('draw the first argument of MATCH as a field', () => {
    const query = "MATCH(body, 'timeout') AND match_text( message, 'refused')";

    expect(wordsDrawnAs(query, 'field.query')).toEqual(['body', 'message']);
  });

  it('leave a name that stands in no field place plain', () => {
    expect(wordsDrawnAs('service_name timeout AND level', 'field.query')).toEqual([]);
  });

  it('read comparisons and conjunctions in any case, as the query does', () => {
    const query = 'level CONTAINS "warn" and status eq 1';

    expect(wordsDrawnAs(query, 'field.query')).toEqual(['level', 'status']);
    expect(wordsDrawnAs(query, 'operator.query')).toEqual(['CONTAINS', 'and', 'eq']);
  });

  it('draw a conjunction before a bracket as one, not as a call', () => {
    const query = 'level = "error" AND (service.name = "a" OR(service.name = "b"))';

    expect(wordsDrawnAs(query, 'function')).toEqual([]);
    expect(wordsDrawnAs(query, 'field.query')).toEqual(['level', 'service.name', 'service.name']);
    expect(wordsDrawnAs(query, 'operator.query').filter((word) => /^[a-z]+$/i.test(word))).toEqual(['AND', 'OR']);
  });
});
