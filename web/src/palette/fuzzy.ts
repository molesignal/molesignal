import fuzzysort from 'fuzzysort';

import { getUsage, KIND_PRIORITY, type ResultItem } from './registry';

interface Searchable {
  item: ResultItem;
  key: string;
}

export function rankResults(query: string, items: ResultItem[]): ResultItem[] {
  const searchables: Searchable[] = items.map((item) => ({
    item,
    key: `${item.label} ${item.subtitle ?? ''} ${item.kind}`.toLowerCase(),
  }));

  if (!query.trim()) {
    // No query — sort by usage recency, then kind priority, then label.
    return [...items].sort((a, b) => {
      const pa = a.priority ?? 0;
      const pb = b.priority ?? 0;
      if (pa !== pb) return pa - pb;
      const ua = getUsage(a.id);
      const ub = getUsage(b.id);
      if (ua !== ub) return ub - ua;
      const ka = KIND_PRIORITY[a.kind] ?? 99;
      const kb = KIND_PRIORITY[b.kind] ?? 99;
      if (ka !== kb) return ka - kb;
      return a.label.localeCompare(b.label);
    });
  }

  const ranked = fuzzysort.go(query, searchables, { key: 'key', limit: 50, threshold: -10000 });
  return ranked
    .map((r) => ({ item: r.obj.item, score: r.score }))
    .sort((a, b) => {
      if (a.score !== b.score) return b.score - a.score;
      const pa = a.item.priority ?? 0;
      const pb = b.item.priority ?? 0;
      if (pa !== pb) return pa - pb;
      const ua = getUsage(a.item.id);
      const ub = getUsage(b.item.id);
      if (ua !== ub) return ub - ua;
      const ka = KIND_PRIORITY[a.item.kind] ?? 99;
      const kb = KIND_PRIORITY[b.item.kind] ?? 99;
      return ka - kb;
    })
    .map((x) => x.item);
}
