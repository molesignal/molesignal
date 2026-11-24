import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { SUPPORTED_LOCALES } from '../../src/i18n/locales';
import {
  argValue,
  buildSystemPrompt,
  chunk,
  concurrencyArg,
  connectProvider,
  fail,
  requestTranslations,
  runPool,
} from './engine';
import { SOURCE_LOCALE, setsEqual, WEB } from './shared';

/**
 * Fills in the `i18n_language` rows the backend lacks: the text the server
 * writes itself (emails, chat-app and notification messages, built-in names),
 * seeded in the baseline migration with one row per key and language.
 *
 * Every key is seeded in en-us and zh-cn, written by hand. This adds a row for
 * each other language in SUPPORTED_LOCALES that the key lacks, right after the
 * key's other rows and in that order, translated from en-us (zh-tw from zh-cn).
 * `backend.state.json` records the source text each row was translated from, so
 * a row whose source changed is translated again; a row with no record yet is
 * kept and recorded. It is safe to re-run.
 *
 * Usage:
 *   pnpm i18n:translate-backend                  # every missing row
 *   pnpm i18n:translate-backend --locale=ja-jp   # one language
 *   pnpm i18n:translate-backend --dry-run        # count what is missing, call no API
 */

const MIGRATION = join(
  WEB,
  '..',
  'crates/engines/postgres/src/migrations/20260101000001_initial.sql',
);

const STATE = join(WEB, 'scripts/i18n/backend.state.json');
const WRITTEN = new Set(['en-us', 'zh-cn']);

const BATCH_SIZE = 20;
const DEFAULT_CONCURRENCY = 6;
const TRANSLATED_FROM: Record<string, string> = { 'zh-tw': 'zh-cn' };
const ROW = /^ {4}\('([a-z0-9_.]+)', '([a-z0-9-]+)', (E?)'(.*)'\),?$/;
// `{name}` fills, and the `{{message.title}}` expressions of notification templates.
const PLACEHOLDER = /\{\{[^{}]*\}\}|\{[a-z0-9_]+\}/g;

interface Row {
  key: string;
  code: string;
  value: string;
}

/** A line of the seed: a row, or a comment or blank line kept as written. */
type Line = { row: Row } | { text: string };

function decode(escaped: boolean, literal: string): string {
  const unquoted = literal.replace(/''/g, "'");
  return escaped ? unquoted.replace(/\\n/g, '\n').replace(/\\\\/g, '\\') : unquoted;
}

function encode(value: string): string {
  const quoted = value.replace(/'/g, "''");
  if (!/[\n\\]/.test(value)) return `'${quoted}'`;
  return `E'${quoted.replace(/\\/g, '\\\\').replace(/\n/g, '\\n')}'`;
}

function fingerprint(text: string): string {
  return createHash('sha256').update(text).digest('hex').slice(0, 16);
}

/** `<key> <language>` → the fingerprint of the source text the row was translated from. */
function readState(): Record<string, string> {
  if (!existsSync(STATE)) return {};
  const state = JSON.parse(readFileSync(STATE, 'utf8')) as { version: number; hashes: Record<string, string> };
  if (state.version !== 1) fail('translate-backend: unsupported backend.state.json.');
  return state.hashes;
}

function writeState(hashes: Record<string, string>): void {
  const sorted = Object.fromEntries(Object.entries(hashes).sort(([a], [b]) => a.localeCompare(b)));
  writeFileSync(STATE, `${JSON.stringify({ version: 1, hashes: sorted }, null, 2)}\n`, 'utf8');
}

function placeholders(text: string): Set<string> {
  return new Set([...text.matchAll(PLACEHOLDER)].map((match) => match[0]));
}

function readSeed(sql: string): { before: string; lines: Line[]; after: string } {
  const insert = sql.indexOf('INSERT INTO i18n_language');
  const open = sql.indexOf('FROM (VALUES\n', insert);
  const close = sql.indexOf(') AS seed(language_key', open);
  if (insert < 0 || open < 0 || close < 0) fail('translate-backend: the i18n_language seed was not found.');

  const start = open + 'FROM (VALUES\n'.length;
  const lines = sql
    .slice(start, close)
    .replace(/\n$/, '')
    .split('\n')
    .map((text): Line => {
      const match = ROW.exec(text);
      if (!match) {
        if (text.trim() !== '' && !text.trim().startsWith('--')) {
          fail(`translate-backend: cannot read the seed line ${JSON.stringify(text)}.`);
        }
        return { text };
      }
      const [, key = '', code = '', escaped = '', literal = ''] = match;
      return { row: { key, code, value: decode(escaped === 'E', literal) } };
    });

  return { before: sql.slice(0, start), lines, after: sql.slice(close) };
}

function writeSeed(before: string, lines: Line[], after: string): string {
  const lastRow = lines.reduce((last, line, index) => ('row' in line ? index : last), -1);
  const body = lines.map((line, index) => {
    if (!('row' in line)) return line.text;
    const { key, code, value } = line.row;
    return `    ('${key}', '${code}', ${encode(value)})${index === lastRow ? '' : ','}`;
  });
  return `${before}${body.join('\n')}\n${after}`;
}

async function main(): Promise<void> {
  const dryRun = process.argv.includes('--dry-run');
  const localeArg = argValue('locale');
  const concurrency = concurrencyArg(DEFAULT_CONCURRENCY);
  const targets = SUPPORTED_LOCALES.filter(
    (locale) => !WRITTEN.has(locale) && (!localeArg || locale === localeArg),
  );
  if (targets.length === 0) fail(`translate-backend: ${localeArg} is not a machine-translated language.`);

  const sql = readFileSync(MIGRATION, 'utf8');
  const { before, lines, after } = readSeed(sql);

  const values = new Map<string, Map<string, string>>();
  for (const line of lines) {
    if (!('row' in line)) continue;
    const byCode = values.get(line.row.key) ?? new Map<string, string>();
    byCode.set(line.row.code, line.row.value);
    values.set(line.row.key, byCode);
  }

  const recorded = readState();
  const hashes: Record<string, string> = {};
  const sourceOf = (key: string, from: string) => values.get(key)?.get(from) ?? values.get(key)?.get(SOURCE_LOCALE);

  const missing = targets.map((locale) => {
    const from = TRANSLATED_FROM[locale] ?? SOURCE_LOCALE;
    const keys: string[] = [];
    let stale = 0;

    for (const key of values.keys()) {
      const source = sourceOf(key, from);
      if (source === undefined) continue;
      const id = `${key} ${locale}`;
      const now = fingerprint(source);

      if (!values.get(key)?.has(locale)) {
        keys.push(key);
      } else if (recorded[id] !== undefined && recorded[id] !== now) {
        keys.push(key);
        stale += 1;
        // Kept until the new translation comes back, so a failed batch is retried.
        hashes[id] = recorded[id];
      } else {
        hashes[id] = now;
      }
    }
    return { locale, from, keys, stale };
  });
  // Other languages' records stay when one language is translated.
  for (const [id, hash] of Object.entries(recorded)) {
    const [key, locale] = id.split(' ');
    if (key && locale && !targets.includes(locale as never) && values.get(key)?.has(locale)) hashes[id] = hash;
  }
  const total = missing.reduce((sum, { keys }) => sum + keys.length, 0);

  if (total === 0) {
    if (!dryRun) writeState(hashes);
    console.log('translate-backend: every row is translated from the current source text.');
    return;
  }
  if (dryRun) {
    for (const { locale, from, keys, stale } of missing) {
      console.log(`${locale}: ${keys.length} row(s) from ${from}${stale ? `, ${stale} of them because the source changed` : ''}`);
    }
    console.log(`\ntranslate-backend: ${total} row(s) would be translated (dry run).`);
    return;
  }

  const provider = await connectProvider();
  console.log(`translate-backend: ${provider.id} (${provider.model}) → ${total} row(s), ${concurrency} at a time`);

  const translated = new Map<string, Map<string, string>>();
  let skipped = 0;
  const jobs = missing.flatMap(({ locale, from, keys }) =>
    chunk(keys, BATCH_SIZE).map((batch) => async () => {
      const texts: Record<string, string> = {};
      for (const key of batch) {
        const text = sourceOf(key, from);
        if (text !== undefined) texts[key] = text;
      }

      let answer: Record<string, unknown>;
      try {
        answer = await requestTranslations(provider, buildSystemPrompt(locale, from), texts);
      } catch (error) {
        console.error(`  ! ${locale}: batch failed (${(error as Error).message}); left out`);
        skipped += batch.length;
        return;
      }

      for (const [key, text] of Object.entries(texts)) {
        const value = answer[key];
        if (typeof value !== 'string' || value.trim() === '' || !setsEqual(placeholders(text), placeholders(value))) {
          console.error(`  ! ${locale}: "${key}" came back without its placeholders; left out`);
          skipped += 1;
          continue;
        }
        const byCode = translated.get(key) ?? new Map<string, string>();
        byCode.set(locale, value);
        translated.set(key, byCode);
        hashes[`${key} ${locale}`] = fingerprint(text);
      }
    }),
  );
  await runPool(jobs, concurrency);

  // Each key's new rows go after its last row, in SUPPORTED_LOCALES order.
  const order = new Map<string, number>(SUPPORTED_LOCALES.map((locale, index) => [locale, index]));
  const lastLineOf = new Map<string, number>();
  lines.forEach((line, index) => {
    if ('row' in line) lastLineOf.set(line.row.key, index);
  });

  const out: Line[] = [];
  lines.forEach((line, index) => {
    out.push(line);
    if (!('row' in line) || lastLineOf.get(line.row.key) !== index) return;

    const key = line.row.key;
    const fresh = translated.get(key) ?? new Map<string, string>();
    const added = [...fresh].filter(([code]) => !values.get(key)?.has(code)).map(([code, value]) => ({ key, code, value }));
    // The key's rows written so far: the run of its rows just before this point.
    let first = out.length;
    while (first > 0) {
      const previous = out[first - 1];
      if (!previous || !('row' in previous) || previous.row.key !== key) break;
      first -= 1;
    }
    const rows = [...out.splice(first), ...added.map((row) => ({ row }))];
    rows.sort((a, b) => {
      const codeOf = (item: Line) => ('row' in item ? item.row.code : '');
      return (order.get(codeOf(a)) ?? 99) - (order.get(codeOf(b)) ?? 99);
    });
    out.push(...rows);
  });

  // A row translated again replaces its old text where it stands.
  for (const line of out) {
    if (!('row' in line)) continue;
    const value = translated.get(line.row.key)?.get(line.row.code);
    if (value !== undefined) line.row.value = value;
  }

  writeFileSync(MIGRATION, writeSeed(before, out, after), 'utf8');
  writeState(hashes);
  console.log(
    `\ntranslate-backend: translated ${total - skipped} row(s)${skipped ? `, ${skipped} left out — run the same command again to retry them` : ''}.`,
  );
  if (skipped > 0) process.exit(1);
}

main().catch((error: unknown) => fail(`translate-backend: ${(error as Error).message}`));
