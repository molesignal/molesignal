import { describe, expect, it } from 'vitest';

import { byAttention, inScope, type ReaderScope, type ResourceRow, scopedRows } from './model';

function row(key: string, fields: Partial<ResourceRow> = {}): ResourceRow {
  return { key, name: key, path: `/${key}`, ownerId: null, teamId: null, starred: false, ...fields };
}

const reader: ReaderScope = {
  userId: 'me',
  teamIds: new Set(['payments']),
  teammateIds: new Set(['me', 'ada']),
};

describe('inScope', () => {
  it('tells mine, my teams’, my favourites and all apart', () => {
    expect(inScope(row('a', { ownerId: 'me' }), 'mine', reader)).toBe(true);
    expect(inScope(row('b', { teamId: 'payments' }), 'team', reader)).toBe(true);
    expect(inScope(row('c', { ownerId: 'ada' }), 'team', reader)).toBe(true);
    expect(inScope(row('d', { ownerId: 'someone' }), 'team', reader)).toBe(false);
    expect(inScope(row('e', { starred: true }), 'starred', reader)).toBe(true);
    expect(inScope(row('f'), 'all', reader)).toBe(true);
  });
});

describe('scopedRows', () => {
  it('keeps the scope chosen when it holds rows', () => {
    const shown = scopedRows([row('a', { ownerId: 'me' }), row('b')], 'mine', reader);

    expect(shown).toMatchObject({ scope: 'mine' });
    expect(shown.rows.map((kept) => kept.key)).toEqual(['a']);
  });

  it('goes on to the next scope that holds rows: teams, then favourites, then all', () => {
    expect(scopedRows([row('a', { teamId: 'payments' })], 'mine', reader).scope).toBe('team');
    expect(scopedRows([row('a', { starred: true })], 'mine', reader).scope).toBe('starred');
    expect(scopedRows([row('a')], 'mine', reader).scope).toBe('all');
    expect(scopedRows([], 'mine', reader)).toEqual({ scope: 'all', rows: [] });
  });
});

describe('byAttention', () => {
  it('puts what is failing first, then the busiest', () => {
    const rows = [
      row('quiet', { state: 'healthy', requestRate: 1 }),
      row('busy', { state: 'healthy', requestRate: 90 }),
      row('broken', { state: 'critical', requestRate: 5 }),
      row('warned', { state: 'warning' }),
    ];

    expect(rows.sort(byAttention).map((kept) => kept.key)).toEqual(['broken', 'warned', 'busy', 'quiet']);
  });
});
