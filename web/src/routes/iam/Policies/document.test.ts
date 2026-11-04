import { describe, expect, it } from 'vitest';

import { actionMatches, documentAllowing, problemMarkers, readPolicy } from './document';

const CATALOG = [
  'alerts.rules.manage',
  'alerts.rules.read',
  'dashboards.edit',
  'dashboards.read',
  'streams.query',
];

const read = (document: unknown) => readPolicy(JSON.stringify(document, null, 2), CATALOG);

describe('readPolicy', () => {
  it('allows what the actions name, patterns included', () => {
    expect(
      read({
        Version: '1',
        Statement: [
          { Sid: 'Read', Effect: 'Allow', Action: ['dashboards.read', 'alerts.*'] },
          { Effect: 'Allow', Action: 'streams.query' },
        ],
      }),
    ).toEqual({
      ok: true,
      permissions: ['alerts.rules.manage', 'alerts.rules.read', 'dashboards.read', 'streams.query'],
      simple: false,
    });
  });

  it('calls a single statement of named permissions simple', () => {
    const reading = readPolicy(documentAllowing(['dashboards.read', 'alerts.rules.read']), CATALOG);

    expect(reading).toEqual({
      ok: true,
      permissions: ['alerts.rules.read', 'dashboards.read'],
      simple: true,
    });
  });

  it('names the first thing wrong, with where it is', () => {
    expect(readPolicy('{', CATALOG)).toEqual({ ok: false, problem: { code: 'json' } });

    expect(
      read({ Version: '1', Statement: [{ Effect: 'Allow', Action: ['dashboards.reed'] }] }),
    ).toMatchObject({
      ok: false,
      problem: { code: 'unknown_action', path: 'Statement[0].Action[0]', value: 'dashboards.reed' },
    });

    expect(
      read({ Version: '1', Statement: [{ Effect: 'Deny', Action: 'alerts.rules.read' }] }),
    ).toMatchObject({ ok: false, problem: { code: 'effect' } });

    expect(
      read({ Version: '1', Statement: [{ Effect: 'Allow', Action: 'alerts.rules.read', Resource: '*' }] }),
    ).toMatchObject({ ok: false, problem: { code: 'unsupported_key', value: 'Resource' } });

    expect(read({ Version: '2', Statement: [] })).toMatchObject({
      ok: false,
      problem: { code: 'version' },
    });

    expect(read({ Version: '1', Statement: {} })).toMatchObject({
      ok: false,
      problem: { code: 'statement_list' },
    });
  });

  it('reads an empty list as allowing nothing, the way a new policy starts', () => {
    const nothing = { ok: true, permissions: [], simple: true };

    expect(readPolicy(documentAllowing([]), CATALOG)).toEqual(nothing);
    expect(read({ Version: '1', Statement: [] })).toEqual(nothing);
    expect(
      read({
        Version: '1',
        Statement: [
          { Effect: 'Allow', Action: [] },
          { Effect: 'Allow', Action: 'alerts.rules.read' },
        ],
      }),
    ).toEqual({ ok: true, permissions: ['alerts.rules.read'], simple: false });
  });

  it('underlines the action at fault', () => {
    const text = documentAllowing(['alerts.rules.read', 'dashboards.reed']);
    const reading = readPolicy(text, CATALOG);

    if (reading.ok) throw new Error('expected a problem');

    expect(problemMarkers(text, reading.problem, 'no such permission')).toEqual([
      {
        line: 8,
        startColumn: 9,
        endColumn: 26,
        message: 'no such permission',
        severity: 'error',
      },
    ]);
  });
});

describe('actionMatches', () => {
  it('matches a pattern as written', () => {
    expect(actionMatches('*.read', 'alerts.rules.read')).toBe(true);
    expect(actionMatches('dash*.e*t', 'dashboards.edit')).toBe(true);
    expect(actionMatches('dashboards.*', 'alerts.rules.read')).toBe(false);
    expect(actionMatches('alerts.rules.read*x', 'alerts.rules.read')).toBe(false);
    expect(actionMatches('*', 'streams.query')).toBe(true);
  });
});
