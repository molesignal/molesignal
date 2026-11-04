import { describe, expect, it } from 'vitest';

import { compositeProblem, expressionLetters, heartbeatExamples, judgesItself, nextLetter, withoutQuery } from './kinds';

describe('composite expressions', () => {
  it('lists the letters used once each, upper-cased', () => {
    expect(expressionLetters('a && (B || !a)')).toEqual(['A', 'B']);
  });

  it('names what keeps an expression from saving', () => {
    expect(compositeProblem({ expression: ' ', rules: {} })).toEqual({ kind: 'empty' });
    expect(compositeProblem({ expression: 'A & B', rules: { A: 'r1', B: 'r2' } })).toEqual({ kind: 'characters' });
    expect(compositeProblem({ expression: 'AB', rules: { A: 'r1' } })).toEqual({ kind: 'characters' });
    expect(compositeProblem({ expression: 'A && C', rules: { A: 'r1', B: 'r2' } })).toEqual({ kind: 'letters', letters: ['C'] });
    expect(compositeProblem({ expression: 'A', rules: { A: 'r1', B: 'r2' } })).toEqual({ kind: 'unused', letters: ['B'] });
    expect(compositeProblem({ expression: '(A || B) && !C', rules: { A: 'r1', B: 'r2', C: 'r3' } })).toBeNull();
  });

  it('offers the first free letter', () => {
    expect(nextLetter({ A: 'r1', C: 'r3' })).toBe('B');
  });
});

describe('rule kinds', () => {
  it('tells the kinds that judge themselves and those without a query', () => {
    expect(judgesItself('forecast')).toBe(true);
    expect(judgesItself('scheduled')).toBe(false);
    expect(withoutQuery('heartbeat')).toBe(true);
    expect(withoutQuery('outlier')).toBe(false);
  });

  it('writes check-in commands for an address', () => {
    expect(heartbeatExamples('https://ms.example.com/api/v1/intake/heartbeats/abc').fail).toContain('?status=fail');
  });
});
