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
 * underlying solid color), unless the pair names the fills the veil lies
 * over: then it is flattened onto each of them and measured there.
 */
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { hex as contrastHex } from 'wcag-contrast';

const HERE = dirname(fileURLToPath(import.meta.url));
const TOKENS_DIR = resolve(HERE, '..', 'src', 'shell');
const SEMANTIC_PATH = resolve(TOKENS_DIR, 'tokens.css');
const BASELINE_PATH = resolve(HERE, 'check-contrast.baseline.json');

// The shipped default palette must meet WCAG AA+ on its own.
const PALETTES = ['default'] as const;
type Palette = (typeof PALETTES)[number];
type Theme = 'dark' | 'light';

interface Pair {
  fg: string;
  bg: string;
  /** The opaque fills `bg` lies over where it is a veil (an rgba tone): it is flattened onto each. */
  over?: string[];
  /** Target ratio: 7 for critical alert text, 4.5 for body, 3 for UI/large. */
  target: 7 | 4.5 | 3;
  /** Human description: "AAA alert" / "AA body" / "AA UI". */
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
  ['--tx-3', '--bg-0'],
  ['--tx-3', '--bg-1'],
  ['--tx-3', '--bg-2'],
  ['--tx-3', '--bg-3'],
  ['--fg', '--surface'],
];
// Status-text pairs: a word in a status color (`text-green`, the state of a check in its row)
// holds AA on every fill a row takes, as ink does.
const STATUS_TONES = ['--green', '--yellow', '--blue', '--purple', '--orange'];
// Ink pairs: a status tone as small text on a row holds AA on every fill a row takes. bg-3 is
// the hardest of them; a selected row (indigo-dim over its surface) stays nearer the surface.
const INK_TONES = ['--blue-ink', '--green-ink', '--orange-ink', '--red-ink', '--purple-ink', '--yellow-ink'];
const ROW_FILLS = ['--bg-0', '--bg-1', '--bg-2', '--bg-3'];
// Tint pairs: a word in ink on the tint of its tone (a chip, a notice), and every ink on the
// indigo tint, the fill of a reference that holds the keyboard (what it names may be set in ink,
// an HTTP method). A tint is opaque on light; on dark it is a veil over the fill of its row.
const TINT_TONES = ['blue', 'green', 'orange', 'purple', 'yellow'];
const FOCUS_TINT = '--indigo-tint';
// The tones in which a chip is set in the status color itself on its tint: a table that has
// rows, a notice that an import went through, a filter that was left out of a query.
const STATUS_ON_TINT = ['green', 'yellow'];
// Critical/firing red is held to AAA on both page and surface backgrounds.
const ALERT_TEXT_PAIRS: Array<[string, string]> = [
  ['--red', '--bg-0'],
  ['--red', '--bg-1'],
  ['--red-soft', '--bg-0'],
  ['--red-soft', '--bg-1'],
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
  for (const fg of STATUS_TONES) for (const bg of ROW_FILLS) out.push({ fg, bg, target: 4.5, label: 'AA body' });
  for (const fg of INK_TONES) for (const bg of ROW_FILLS) out.push({ fg, bg, target: 4.5, label: 'AA body' });
  for (const tone of TINT_TONES) {
    out.push({ fg: `--${tone}-ink`, bg: `--${tone}-tint`, over: ROW_FILLS, target: 4.5, label: 'AA body' });
  }
  for (const tone of STATUS_ON_TINT) {
    out.push({ fg: `--${tone}`, bg: `--${tone}-tint`, over: ROW_FILLS, target: 4.5, label: 'AA body' });
  }
  for (const fg of INK_TONES) out.push({ fg, bg: FOCUS_TINT, over: ROW_FILLS, target: 4.5, label: 'AA body' });
  for (const [fg, bg] of ALERT_TEXT_PAIRS) out.push({ fg, bg, target: 7, label: 'AAA alert' });
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
  // Light themes inherit unset tokens from their dark theme, matching the
  // `[data-theme='light']` CSS cascade.
  for (const p of PALETTES) {
    for (const [name, value] of result[p].dark.entries()) {
      if (!result[p].light.has(name)) result[p].light.set(name, value);
    }
  }
  // Resolve var() references transitively per bucket.
  for (const p of PALETTES) {
    for (const theme of ['dark', 'light'] as Theme[]) {
      const table = result[p][theme];
      const resolved = new Map<string, string>();
      for (const name of table.keys()) {
        const paint = resolvePaint(name, table);
        if (paint) resolved.set(name, paint);
      }
      result[p][theme] = resolved;
    }
  }
  return result;
}

/** A token's value with its `var()` chain followed: a hex colour, or an `rgba()` veil as written. */
function resolvePaint(name: string, table: Map<string, string>, seen = new Set<string>()): string | null {
  if (seen.has(name)) return null;
  seen.add(name);
  const raw = table.get(name);
  if (!raw) return null;
  const varMatch = raw.match(/^var\((--[a-z0-9-]+)\)$/i);
  if (varMatch) return resolvePaint(varMatch[1]!, table, seen);
  const hexMatch = raw.match(/^#([0-9a-f]{3,8})$/i);
  if (hexMatch) return `#${hexMatch[1]}`;
  if (VEIL.test(raw)) return raw;
  return null;
}

const VEIL = /^rgba\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*([\d.]+)\s*\)$/i;

function isHex(paint: string): boolean {
  return paint.startsWith('#');
}

/** The colour a veil shows over an opaque fill. */
function flatten(veil: string, underHex: string): string | null {
  const parts = veil.match(VEIL);
  const under = underHex.match(/^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i);
  if (!parts || !under) return null;
  const alpha = Number(parts[4]);
  const channels = [1, 2, 3].map((index) => {
    const over = Number(parts[index]);
    const below = Number.parseInt(under[index]!, 16);
    return Math.round(over * alpha + below * (1 - alpha))
      .toString(16)
      .padStart(2, '0');
  });
  return `#${channels.join('')}`;
}

interface Result {
  palette: Palette;
  theme: Theme;
  pair: Pair;
  /** The fill a veiled background was flattened onto. */
  over?: string;
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
        const bg = table.get(pair.bg);
        if (!fgHex || !bg || !isHex(fgHex)) continue;
        if (isHex(bg)) {
          results.push({ palette, theme, pair, ratio: contrastHex(fgHex, bg), fgHex, bgHex: bg });
          continue;
        }

        for (const over of pair.over ?? []) {
          const under = table.get(over);
          const bgHex = under && isHex(under) ? flatten(bg, under) : null;
          if (!bgHex) continue;
          results.push({ palette, theme, pair, over, ratio: contrastHex(fgHex, bgHex), fgHex, bgHex });
        }
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
        const key = `${palette}|${theme}|${r.pair.fg}|${r.pair.bg}${r.over ? `|${r.over}` : ''}`;
        const on = `${theme}.${r.pair.bg}${r.over ? ` over ${theme}.${r.over}` : ''}`;
        const inBaseline = baseline.has(key);
        const status = ok ? 'OK  ' : inBaseline ? 'WARN' : 'FAIL';
        console.log(
          `  ${status} ${theme}.${r.pair.fg} ON ${on}: ${formatRatio(r.ratio)}:1 ` +
            `(target ${r.pair.target.toFixed(1)}:1, ${r.pair.label}) — ${r.fgHex} on ${r.bgHex}`,
        );
        if (!ok) {
          if (inBaseline) {
            knownFails += 1;
            stillFailing.add(key);
          } else {
            newFails += 1;
            process.stderr.write(
              `FAIL ${palette}.${theme}.${r.pair.fg} ON ${r.pair.bg}${r.over ? ` over ${r.over}` : ''}: ${formatRatio(r.ratio)}:1 < ${r.pair.target.toFixed(1)}:1 (WCAG ${r.pair.label})\n`,
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
