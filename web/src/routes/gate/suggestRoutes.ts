/** How alike two words of an address are, 0 to 1: the same, one inside the other, or a typo apart. */
function likeness(a: string, b: string): number {
  if (a === b) return 1;

  // One inside the other counts when it is most of it: `silence` in `silences`, not `not` in `notify`.
  const [short, long] = a.length < b.length ? [a, b] : [b, a];
  if (short.length >= 4 && short.length / long.length >= 0.6 && long.includes(short)) return 0.8;

  const distance = editDistance(a, b);
  const similar = 1 - distance / Math.max(a.length, b.length);
  return similar >= 0.6 ? similar : 0;
}

function editDistance(a: string, b: string): number {
  let previous = Array.from({ length: b.length + 1 }, (_, index) => index);

  for (let i = 1; i <= a.length; i += 1) {
    const current = [i];
    for (let j = 1; j <= b.length; j += 1) {
      current[j] = Math.min(
        (previous[j] ?? 0) + 1,
        (current[j - 1] ?? 0) + 1,
        (previous[j - 1] ?? 0) + (a[i - 1] === b[j - 1] ? 0 : 1),
      );
    }
    previous = current;
  }

  return previous[b.length] ?? 0;
}

function wordsOf(path: string): string[] {
  return path
    .toLowerCase()
    .split(/[/_-]+/)
    .filter(Boolean);
}

/**
 * The pages an address that leads nowhere may have meant, likeliest first: the
 * ones sharing the most of its words, a typo allowed. Nothing close, nothing said.
 */
export function suggestRoutes<T extends { path: string }>(pathname: string, routes: readonly T[], limit = 4): T[] {
  const words = wordsOf(pathname);
  if (words.length === 0) return [];

  return routes
    .map((route) => {
      const own = wordsOf(route.path);
      const score = words.reduce((sum, word) => sum + Math.max(0, ...own.map((other) => likeness(word, other))), 0);
      return { route, score, length: own.length };
    })
    // Half of what was asked for has to be there: one word in common among five says nothing.
    .filter((entry) => entry.score >= 0.6 && entry.score >= words.length / 2)
    .sort((a, b) => b.score - a.score || a.length - b.length)
    .slice(0, limit)
    .map((entry) => entry.route);
}
