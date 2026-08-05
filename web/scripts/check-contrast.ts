/**
 * WCAG 2.1 AA contrast gate
 * (web-a11y-baseline + web-theming-i18n).
 *
 * Parses `web/src/shell/tokens.css` (semantic alias layer) and the three
 * palette files `tokens-palette-default.css` / `-high-contrast.css` /
 * `-warm.css` for every `palette × theme` pair, resolves `var(...)`
 * aliases to concrete hex values, and prints a contrast report. Exits
 * non-zero when any active pair falls below WCAG AA — 4.5:1 for body
 * text, 3:1 for `*-fg on *-bg` accent badges.
 *
 * Tokens with `rgba(...)` are skipped (alpha makes ratio environment-
 * dependent — the comparable pair is reported separately via the
 * underlying solid color).
 */
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { hex as contrastHex } from 'wcag-contrast';

const HERE = dirname(fileURLToPath(import.meta.url));
const TOKENS_DIR = resolve(HERE, '..', 'src', 'shell');
const SEMANTIC_PATH = resolve(TOKENS_DIR, 'tokens.css');
const BASELINE_PATH = resolve(HERE, 'check-contrast.baseline.json');

// Phase 4: collapsed to a single palette. `high-contrast` and `warm`
// were retired; the default palette must hit WCAG AA+ on its own.
const PALETTES = ['default'] as const;
type Palette = (typeof PALETTES)[number];
type Theme = 'dark' | 'light';

interface Pair {
  fg: string;
  bg: string;
  /** Target ratio: 4.5 for body, 3 for UI/large. */
  target: 4.5 | 3;
  /** Human description: "AA body" / "AA UI". */
  label: string;
}

// Body-text pairs (require AA 4.5:1).
const BODY_PAIRS: Array<[string, string]> = [
  ['--tx-0', '--bg-0'],
  ['--tx-0', '--bg-1'],
  ['--tx-0', '--bg-2'],
  ['--tx-1', '--bg-0'],
  ['--tx-1', '--bg-1'],
  ['--tx-2', '--bg-0'],
  ['--tx-2', '--bg-1'],
  ['--fg', '--surface'],
];
// Status-text pairs (color word on the chrome surface, AA 4.5).
const STATUS_TEXT_PAIRS: Array<[string, string]> = [
  ['--red', '--bg-0'],
  ['--green', '--bg-0'],
  ['--yellow', '--bg-0'],
  ['--blue', '--bg-0'],
  ['--purple', '--bg-0'],
  ['--orange', '--bg-0'],
];
// Accent-badge pairs (UI element; AA Large 3:1).
const BADGE_PAIRS: Array<[string, string]> = [
  ['--accent-fg', '--accent'],
  ['--primary-fg', '--primary'],
  ['--red-fg', '--red'],
  ['--green-fg', '--green'],
  ['--yellow-fg', '--yellow'],
  ['--blue-fg', '--blue'],
  ['--purple-fg', '--purple'],
];

function buildPairList(): Pair[] {
  const out: Pair[] = [];
  for (const [fg, bg] of BODY_PAIRS) out.push({ fg, bg, target: 4.5, label: 'AA body' });
  for (const [fg, bg] of STATUS_TEXT_PAIRS) out.push({ fg, bg, target: 4.5, label: 'AA body' });
  for (const [fg, bg] of BADGE_PAIRS) out.push({ fg, bg, target: 3, label: 'AA UI' });
  return out;
}

interface CssBlock {
  selector: string;
  body: string;
}

function splitBlocks(css: string): CssBlock[] {
  const out: CssBlock[] = [];
  let i = 0;
  while (i < css.length) {
    const open = css.indexOf('{', i);
    if (open < 0) break;
    const selector = css.slice(i, open).trim();
    if (selector.startsWith('@')) {
      // skip @import, @media outer; we don't recurse — palette files don't
      // wrap variables in @media.
      const close = css.indexOf('}', open);
      i = close + 1;
      continue;
    }
    const close = css.indexOf('}', open);
    if (close < 0) break;
    const body = css.slice(open + 1, close);
    out.push({ selector, body });
    i = close + 1;
  }
  return out;
}

/**
 * Match the selector to a (palette, theme) bucket so we know which
 * variables to assign to which combo. Selectors of the form
 * `[data-palette='X'][data-theme='Y']` are the most specific; we also
 * pick up legacy `:root, [data-theme='dark']` blocks as the
 * default-palette dark bucket so we keep parsing the old layout.
 */
function bucketsForSelector(selector: string): Array<{ palette: Palette; theme: Theme }> {
  // A selector may list multiple comma-separated forms; we bucket each part
  // independently so `:root[data-palette='warm'], [data-palette='warm'][data-theme='dark']`
  // both resolve to warm/dark but a separate `[data-palette='warm'][data-theme='light']`
  // block only lands in warm/light.
  const parts = selector.split(',').map((p) => p.trim());
  const seen = new Set<string>();
  const out: Array<{ palette: Palette; theme: Theme }> = [];
  for (const part of parts) {
    for (const palette of PALETTES) {
      const paletteSel = `[data-palette='${palette}']`;
      const hasLight = part.includes(`[data-theme='light']`);
      const hasDark = part.includes(`[data-theme='dark']`);
      let theme: Theme | null = null;
      if (hasLight && !hasDark) theme = 'light';
      else if (hasDark && !hasLight) theme = 'dark';
      else if (!hasLight && !hasDark) {
        // No theme qualifier — what palette is it scoped to?
        if (part.includes(paletteSel)) {
          theme = 'dark';
        } else if (palette === 'default' && (part.includes(':root') || part === '')) {
          theme = 'dark';
        }
      }
      if (theme === null) continue;
      // Confirm this part actually targets this palette
      const targetsThis =
        part.includes(paletteSel) ||
        (palette === 'default' &&
          (part.includes(':root') ||
            part === `[data-theme='dark']` ||
            part === `[data-theme='light']`));
      if (!targetsThis) continue;
      const key = `${palette}|${theme}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push({ palette, theme });
    }
  }
  return out;
}

/**
 * Parse CSS files into a `(palette, theme) → token → rawValue` map.
 */
function parseTokensAcrossFiles(files: string[]): Record<Palette, Record<Theme, Map<string, string>>> {
  const result: Record<Palette, Record<Theme, Map<string, string>>> = {
    default: { dark: new Map(), light: new Map() },
  };
  for (const file of files) {
    const css = readFileSync(file, 'utf8');
    for (const { selector, body } of splitBlocks(css)) {
      const buckets = bucketsForSelector(selector);
      if (buckets.length === 0) continue;
      for (const m of body.matchAll(/(--[a-z0-9-]+):\s*([^;]+);/gi)) {
        const name = m[1]!;
        const raw = m[2]!.trim();
        for (const { palette, theme } of buckets) {
          result[palette][theme].set(name, raw);
        }
      }
    }
  }
  // Cascade fallbacks:
  //   - For each non-default palette, fall back to default-dark for unset
  //     dark tokens AND default-light for unset light tokens (palette files
  //     only override the 9 palette slots, not bg/bd/tx).
  //   - For each (palette, light), fall back to (palette, dark) for any
  //     unset tokens — matches `[data-theme='light']` CSS cascade.
  for (const p of PALETTES) {
    for (const [name, value] of result[p].dark.entries()) {
      if (!result[p].light.has(name)) result[p].light.set(name, value);
    }
  }
  // Phase 4: only the `default` palette ships. The cross-palette
  // fallback loop (`for p !== 'default' …`) was removed alongside the
  // retired warm + high-contrast palettes — leaving it produced TS
  // "never" errors because the union collapsed to a single member.
  // Resolve var() references transitively per bucket.
  for (const p of PALETTES) {
    for (const theme of ['dark', 'light'] as Theme[]) {
      const table = result[p][theme];
      const resolved = new Map<string, string>();
      for (const name of table.keys()) {
        const hex = resolveHex(name, table);
        if (hex) resolved.set(name, hex);
      }
      result[p][theme] = resolved;
    }
  }
  return result;
}

function resolveHex(name: string, table: Map<string, string>, seen = new Set<string>()): string | null {
  if (seen.has(name)) return null;
  seen.add(name);
  const raw = table.get(name);
  if (!raw) return null;
  const varMatch = raw.match(/^var\((--[a-z0-9-]+)\)$/i);
  if (varMatch) return resolveHex(varMatch[1]!, table, seen);
  const hexMatch = raw.match(/^#([0-9a-f]{3,8})$/i);
  if (hexMatch) return `#${hexMatch[1]}`;
  return null;
}

interface Result {
  palette: Palette;
  theme: Theme;
  pair: Pair;
  ratio: number;
  fgHex: string;
  bgHex: string;
}

function evaluate(
  tokens: Record<Palette, Record<Theme, Map<string, string>>>,
  pairs: Pair[],
): Result[] {
  const results: Result[] = [];
  for (const palette of PALETTES) {
    for (const theme of ['dark', 'light'] as Theme[]) {
      const table = tokens[palette][theme];
      for (const pair of pairs) {
        const fgHex = table.get(pair.fg);
        const bgHex = table.get(pair.bg);
        if (!fgHex || !bgHex) continue;
        const ratio = contrastHex(fgHex, bgHex);
        results.push({ palette, theme, pair, ratio, fgHex, bgHex });
      }
    }
  }
  return results;
}

function formatRatio(r: number): string {
  return r.toFixed(2);
}

interface BaselineEntry {
  palette?: Palette;
  theme: Theme;
  fg: string;
  bg: string;
  note?: string;
}

function loadBaseline(): Set<string> {
  try {
    const raw = JSON.parse(readFileSync(BASELINE_PATH, 'utf8')) as {
      knownFailures: BaselineEntry[];
    };
    return new Set(
      raw.knownFailures.map((e) => `${e.palette ?? 'default'}|${e.theme}|${e.fg}|${e.bg}`),
    );
  } catch {
    return new Set();
  }
}

function main(): void {
  const files = [
    SEMANTIC_PATH,
    resolve(TOKENS_DIR, 'tokens-palette-default.css'),
  ];
  const tokens = parseTokensAcrossFiles(files);
  const pairs = buildPairList();
  const results = evaluate(tokens, pairs);
  const baseline = loadBaseline();

  let newFails = 0;
  let knownFails = 0;
  const stillFailing = new Set<string>();
  console.log('# WCAG 2.1 AA contrast report (palette × theme matrix)\n');
  for (const palette of PALETTES) {
    for (const theme of ['dark', 'light'] as Theme[]) {
      console.log(`## ${palette} / ${theme}`);
      for (const r of results.filter((x) => x.palette === palette && x.theme === theme)) {
        const ok = r.ratio >= r.pair.target;
        const key = `${palette}|${theme}|${r.pair.fg}|${r.pair.bg}`;
        const inBaseline = baseline.has(key);
        const status = ok ? 'OK  ' : inBaseline ? 'WARN' : 'FAIL';
        console.log(
          `  ${status} ${theme}.${r.pair.fg} ON ${theme}.${r.pair.bg}: ${formatRatio(r.ratio)}:1 ` +
            `(target ${r.pair.target.toFixed(1)}:1, ${r.pair.label}) — ${r.fgHex} on ${r.bgHex}`,
        );
        if (!ok) {
          if (inBaseline) {
            knownFails += 1;
            stillFailing.add(key);
          } else {
            newFails += 1;
            process.stderr.write(
              `FAIL ${palette}.${theme}.${r.pair.fg} ON ${r.pair.bg}: ${formatRatio(r.ratio)}:1 < ${r.pair.target.toFixed(1)}:1 (WCAG ${r.pair.label})\n`,
            );
          }
        }
      }
      console.log();
    }
  }
  const pass = results.length - newFails - knownFails;
  console.log(
    `\n# Summary: ${pass} pass, ${knownFails} known-pre-existing (allowlisted), ${newFails} new failure(s).`,
  );
  const obsolete: string[] = [];
  for (const entry of baseline) {
    if (!stillFailing.has(entry)) obsolete.push(entry);
  }
  if (obsolete.length > 0) {
    console.log(
      `\n# Allowlist drift: ${obsolete.length} entry/entries no longer failing — remove from check-contrast.baseline.json:`,
    );
    for (const o of obsolete) console.log(`  - ${o.replace(/\|/g, ' / ')}`);
  }
  if (newFails > 0) {
    console.error(`\n${newFails} NEW contrast failure(s). See FAIL lines above.`);
    process.exit(1);
  }
}

main();
