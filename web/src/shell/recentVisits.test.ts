import { describe, expect, it } from 'vitest';

import { MAX_RECENT_VISITS, parseRecentVisits, type RecentVisit, withVisit } from './recentVisits';

function visit(id: string, fields: Partial<RecentVisit> = {}): RecentVisit {
  return { kind: 'dashboard', id, label: id, path: `/dashboards/${id}`, visitedAt: 1, ...fields };
}

describe('withVisit', () => {
  it('puts the visit first and takes the same place out further down', () => {
    const visits = withVisit([visit('a'), visit('b'), visit('c')], visit('b', { visitedAt: 9 }));

    expect(visits.map((kept) => kept.id)).toEqual(['b', 'a', 'c']);
    expect(visits[0]?.visitedAt).toBe(9);
  });

  it('keeps places of another kind with the same id apart', () => {
    const visits = withVisit([visit('a')], visit('a', { kind: 'incident', path: '/alerts/incidents/a' }));

    expect(visits.map((kept) => kept.kind)).toEqual(['incident', 'dashboard']);
  });

  it('cuts the list to its length', () => {
    const many = Array.from({ length: MAX_RECENT_VISITS }, (_, index) => visit(`v${index}`));

    expect(withVisit(many, visit('new'))).toHaveLength(MAX_RECENT_VISITS);
  });
});

describe('parseRecentVisits', () => {
  it('keeps the visits it knows and leaves out the rest', () => {
    const parsed = parseRecentVisits({
      visits: [visit('a'), { ...visit('b'), kind: 'unknown' }, { ...visit('c'), path: '//elsewhere.example' }],
    });

    expect(parsed?.visits.map((kept) => kept.id)).toEqual(['a']);
  });

  it('reads anything else as nothing kept', () => {
    expect(parseRecentVisits(null)).toBeNull();
    expect(parseRecentVisits({ visits: 'a' })).toBeNull();
  });
});
