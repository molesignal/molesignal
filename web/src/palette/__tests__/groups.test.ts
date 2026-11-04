import { describe, expect, it } from 'vitest';

import { groupResults, GROUP_LIMIT } from '@/palette/groups';
import type { ItemKind, ResultItem } from '@/palette/registry';

const item = (kind: ItemKind, id: string): ResultItem => ({ kind, id, label: id });

describe('groupResults', () => {
  it('keeps a kind where its best match stood', () => {
    const groups = groupResults(
      [item('service', 'checkout'), item('action', 'go'), item('service', 'payments')],
      new Set(),
    );

    expect(groups.map((group) => group.kind)).toEqual(['service', 'action']);
    expect(groups[0]?.items.map((row) => row.id)).toEqual(['checkout', 'payments']);
  });

  it('shows the first few of a long kind and says how many are behind them', () => {
    const many = Array.from({ length: GROUP_LIMIT + 3 }, (_, index) => item('dashboard', `d${index}`));
    const [group] = groupResults(many, new Set());

    expect(group?.items).toHaveLength(GROUP_LIMIT);
    expect(group?.hidden).toBe(3);
  });

  it('shows them all once asked', () => {
    const many = Array.from({ length: GROUP_LIMIT + 3 }, (_, index) => item('dashboard', `d${index}`));
    const [group] = groupResults(many, new Set<ItemKind>(['dashboard']));

    expect(group?.items).toHaveLength(GROUP_LIMIT + 3);
    expect(group?.hidden).toBe(0);
  });
});
