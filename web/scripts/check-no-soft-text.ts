/**
 * Gate against a soft tone used as text.
 *
 * On the light theme `--blue-soft`, `--green-soft`, `--orange-soft`,
 * `--purple-soft` and `--yellow-soft` are fills: as small text they reach 3.2
 * to 4.2:1 on white and less on a tinted row, under the 4.5:1 of WCAG AA. A
 * word or an icon in a status tone takes ink (`text-[var(--green-ink)]`), and
 * the fill it stands on is a tint (`bg-[var(--green-tint)]`). Both tiers are
 * in `src/shell/tokens-palette-default.css` and held to AA by
 * `check-contrast.ts`.
 *
 * `--red-soft` and `--indigo-soft` are outside the rule: each is a text tone
 * on both themes.
 *
 * A text class also colours what is drawn in `currentColor`. Where that is a
 * mark of a chart and not a word or an icon, the soft tone is the right one:
 * such a line is listed in KEPT with its reason.
 *
 * Run via `pnpm -C web exec tsx scripts/check-no-soft-text.ts` (also wired
 * into `pnpm lint`).
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const WEB = resolve(HERE, '..');
const SRC = join(WEB, 'src');

const TONES = 'blue|green|orange|purple|yellow';

const PATTERNS: Array<{ name: string; re: RegExp }> = [
  // The utility, with any variant in front of it (`hover:`, `focus-visible:`) and any opacity after.
  { name: 'text-*-soft', re: new RegExp(`(?<![\\w-])text-(${TONES})-soft(?![\\w-])`) },
  // The semantic alias of the same tone (`--green-muted` is `--green-soft`).
  { name: 'text-*-muted', re: new RegExp(`(?<![\\w-])text-(${TONES})-muted(?![\\w-])`) },
  // A stylesheet rule or an inline style.
  { name: 'color: var(--*-soft)', re: new RegExp(`(?<![\\w-])color\\s*:\\s*['"\`]?var\\(--(${TONES})-(soft|muted)\\)`) },
];

/** Lines that keep a soft tone on purpose. Each entry has to match a line, or it has gone stale. */
const KEPT: Array<{ file: string; line: string; why: string }> = [
  {
    file: 'src/routes/apm/RedReadouts.tsx',
    line: 'trendClassName="text-orange-soft"',
    why: 'the line of a sparkline, drawn in currentColor',
  },
  {
    file: 'src/routes/metrics/ExemplarRail.tsx',
    line: '<Diamond className="h-3.5 w-3.5 shrink-0 text-purple-soft" aria-hidden />',
    why: 'the mark that keys the exemplars on the rail',
  },
  {
    file: 'src/routes/metrics/ExemplarRail.tsx',
    line: 'rounded text-purple-soft outline-none hover:bg-purple-dim hover:text-purple',
    why: 'an exemplar on the rail: a data mark, drawn in currentColor',
  },
];

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const s = statSync(p);
    if (s.isDirectory()) walk(p, out);
    else if (/\.(tsx?|css)$/.test(name)) out.push(p);
  }
  return out;
}

function main(): number {
  const files = walk(SRC);
  const used = new Set<number>();
  let hits = 0;
  for (const f of files) {
    const file = relative(WEB, f);
    const lines = readFileSync(f, 'utf8').split('\n');
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i]!;
      const pattern = PATTERNS.find(({ re }) => re.test(line));
      if (!pattern) continue;
      const kept = KEPT.findIndex((entry) => entry.file === file && line.includes(entry.line));
      if (kept >= 0) {
        used.add(kept);
        continue;
      }
      console.error(
        `${file}:${i + 1}  ${pattern.name} — a soft tone is a fill; set text in ink (text-[var(--<tone>-ink)])`,
      );
      hits++;
    }
  }
  const stale = KEPT.filter((_, index) => !used.has(index));
  for (const entry of stale) {
    console.error(`${entry.file}  KEPT entry matches no line — remove it: ${entry.line}`);
  }
  if (hits > 0 || stale.length > 0) {
    console.error(`\n${hits} soft-tone text use(s), ${stale.length} stale KEPT entry/entries.`);
    return 1;
  }
  console.log(`check-no-soft-text: ${files.length} files, 0 violations, ${KEPT.length} kept on purpose.`);
  return 0;
}

process.exit(main());
