import {
  logLevelLabel,
  logLevelTone,
  logSourceLabel,
  logStatement,
  type LogEntry,
  type LogLevelTone,
} from '../viewModel';

/** What a part of a message that changes from log to log is, where it can be told. */
export type PatternWild = 'number' | 'id' | 'ip' | 'url' | 'time' | 'data' | 'any';

/** A word of a pattern: said by every log of it, or a place where each says something of its own. */
export interface PatternToken {
  /** Punctuation the word carries before and after it, which is the same in every log. */
  lead: string;
  trail: string;
  /** The word itself; for a place that changes, a sample of what stands there. */
  text: string;
  wild: PatternWild | null;
  /** What follows a number without a space: the unit it is counted in. */
  unit: string;
}

export interface LogPattern {
  /** The pattern as text, a changing place written as its kind. */
  key: string;
  tokens: PatternToken[];
  count: number;
  /** Its share of the logs that say something, 0 to 1. */
  share: number;
  /** The loudest level among its logs. */
  tone: LogLevelTone;
  levels: { level: string; count: number }[];
  sources: { name: string; count: number }[];
  /** Where its logs are in the loaded rows, in their order. */
  rows: number[];
  /** The longest run of words every log of the pattern has: what a query can ask for. */
  fragment: string;
  /** The field the logs say it in. */
  field: string;
}

export interface LogPatterns {
  patterns: LogPattern[];
  /** How many of the logs say something, which the shares are of. */
  read: number;
  /** How many carry no message, and so are in no pattern. */
  unread: number;
}

/** A message is read to its first line break, and no further than this many words. */
const MAX_WORDS = 48;
/** Words that differ at one place are taken for one pattern from this many apart. */
const MERGE_FROM = 3;
/** A pattern shorter than this is an event of its own, not a sentence with a slot in it. */
const MERGE_MIN_WORDS = 3;
/** Shorter than this, a run of words would match far more than the pattern. */
const MIN_FRAGMENT_LENGTH = 4;

const TONE_RANK: Record<LogLevelTone, number> = { error: 3, warn: 2, info: 1, quiet: 0 };
const LEAD = /^[([{"'<]+/;
const TRAIL = /[)\]}"'>,;:.!?]+$/;
const KEY_VALUE = /^([A-Za-z_][\w.-]*[=:])(.+)$/;
const URL = /^[a-z][a-z0-9+.-]*:\/\/\S+$/i;
const IP = /^(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?$/;
const TIME = /^(?:\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:Z|[+-]\d{2}:?\d{2})?)?|\d{2}:\d{2}:\d{2}(?:\.\d+)?)$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const HEX = /^(?:0x)?[0-9a-f]{8,}$/i;
/**
 * A number, and what it is counted in when that is written against it: a time, a
 * size, a rate, a share. Other letters after digits are not a unit: `12ae` is
 * an identifier, not twelve of something.
 */
const NUMBER = /^[-+]?\d[\d,]*(?:\.\d+)?(%|[nuµμm]?s|min|[hd]|[kmgtp]?i?b(?:\/s)?|[kmg]|x|px|rps|qps|hz)?$/i;
/** An object or a list written out, as against a word in brackets such as `[worker]`. */
const DATA_START = /^(?:\{["'}]|\[[{"\d\]])/;

/**
 * The kinds of message among the loaded logs.
 *
 * A message is read word by word. A word that is a number, an identifier, an
 * address or a moment, or that has a digit in it, is taken for a place where
 * each log says something of its own; the words left are the pattern. Then
 * patterns of one length that differ at a single place, in three ways or more,
 * are one pattern with that place open: "user alice signed in" and the same of
 * bob and carol. Two ways are left as two patterns, since "order accepted" and
 * "order rejected" are different events. Where some logs already have a value
 * at that place, a word found there is one more value: an identifier that
 * happens to be all letters among identifiers that are not.
 */
export function logPatterns(rows: readonly LogEntry[]): LogPatterns {
  const groups = new Map<string, { tokens: PatternToken[]; field: string; rows: number[] }>();
  let unread = 0;

  rows.forEach((log, index) => {
    const statement = logStatement(log.raw);
    if (!statement) {
      unread += 1;
      return;
    }

    const tokens = patternTokens(statement.value);
    const key = patternKey(tokens);
    const group = groups.get(key);
    if (group) group.rows.push(index);
    else groups.set(key, { tokens, field: statement.field, rows: [index] });
  });

  const merged = mergeBySingleSlot([...groups.values()]);
  const read = rows.length - unread;
  const patterns = merged.map((group) => summarize(group, rows, read));

  patterns.sort(
    (left, right) =>
      right.count - left.count || TONE_RANK[right.tone] - TONE_RANK[left.tone] || left.key.localeCompare(right.key),
  );

  return { patterns, read, unread };
}

/** The words of a message, each said to be fixed or changing. */
export function patternTokens(message: string): PatternToken[] {
  const line = (message.split(/\r?\n/, 1)[0] ?? '').trim();
  const words = line.split(/\s+/).filter(Boolean);
  const tokens: PatternToken[] = [];

  for (const [index, word] of words.entries()) {
    if (index === MAX_WORDS) {
      tokens.push(fixed('…'));
      break;
    }
    // Structured data set into a message runs to its end, and is never the same twice.
    if (DATA_START.test(word) && /[}\]]$/.test(line)) {
      tokens.push({ lead: '', trail: '', text: words.slice(index).join(' '), wild: 'data', unit: '' });
      break;
    }

    tokens.push(patternToken(word));
  }

  return tokens;
}

function patternToken(word: string): PatternToken {
  const lead = LEAD.exec(word)?.[0] ?? '';
  const rest = word.slice(lead.length);
  const trail = TRAIL.exec(rest)?.[0] ?? '';
  const core = rest.slice(0, rest.length - trail.length);
  if (!core) return fixed(word);

  const wild = wildKind(core);
  // An address or a moment has a colon of its own: it is one thing, not a name and a value.
  if (wild && wild !== 'any') return { lead, trail, text: core, wild, unit: wild === 'number' ? unitOf(core) : '' };

  // `sku=SKU-48213`: the name is the pattern's, the value is the log's own.
  const pair = KEY_VALUE.exec(core);
  const value = pair ? stripQuotes(pair[2]!) : '';
  const valueWild = pair ? wildKind(value) : null;
  if (pair && valueWild) {
    return {
      lead: lead + pair[1]!,
      trail,
      text: value,
      wild: valueWild,
      unit: valueWild === 'number' ? unitOf(value) : '',
    };
  }

  return { lead, trail, text: core, wild, unit: '' };
}

function wildKind(core: string): PatternWild | null {
  if (URL.test(core)) return 'url';
  if (IP.test(core)) return 'ip';
  if (TIME.test(core)) return 'time';
  if (UUID.test(core) || (HEX.test(core) && /\d/.test(core))) return 'id';
  if (NUMBER.test(core)) return 'number';

  // What has a digit in it is, far more often than not, a value and not a word.
  return /\d/.test(core) ? 'any' : null;
}

function unitOf(core: string): string {
  return NUMBER.exec(core)?.[1] ?? '';
}

function stripQuotes(value: string): string {
  return value.replace(/^["']|["']$/g, '');
}

function fixed(text: string): PatternToken {
  return { lead: '', trail: '', text, wild: null, unit: '' };
}

/** A word as the pattern writes it: itself, or the kind of thing that stands in its place. */
function tokenKey(token: PatternToken): string {
  return `${token.lead}${token.wild ? `<${token.wild}>${token.unit}` : token.text}${token.trail}`;
}

export function patternKey(tokens: readonly PatternToken[]): string {
  return tokens.map(tokenKey).join(' ');
}

type Group = { tokens: PatternToken[]; field: string; rows: number[] };

function mergeBySingleSlot(groups: Group[]): Group[] {
  let current = groups;
  const longest = Math.max(0, ...groups.map((group) => group.tokens.length));

  // The first word names the event: it is never opened.
  for (let slot = 1; slot < longest; slot += 1) {
    const buckets = new Map<string, Group[]>();
    const next: Group[] = [];

    for (const group of current) {
      const token = group.tokens[slot];
      if (!token || group.tokens.length < MERGE_MIN_WORDS) {
        next.push(group);
        continue;
      }

      const signature = `${group.field}\u0000${group.tokens
        .map((item, index) => (index === slot ? `${item.lead}\u0001${item.trail}` : tokenKey(item)))
        .join(' ')}`;
      const bucket = buckets.get(signature);
      if (bucket) bucket.push(group);
      else buckets.set(signature, [group]);
    }

    for (const bucket of buckets.values()) {
      const open = bucket.filter((group) => group.tokens[slot]!.wild);

      // Alone there is nothing to join; words alone are one pattern only from three apart.
      if (bucket.length < 2 || (open.length === 0 && bucket.length < MERGE_FROM)) {
        next.push(...bucket);
        continue;
      }

      // The pattern that already has a value there keeps its sample of one; its kind holds only if all agree.
      const first = open[0] ?? bucket[0]!;
      const kinds = new Set(bucket.map((group) => group.tokens[slot]!.wild));
      const wild = kinds.size === 1 ? first.tokens[slot]!.wild ?? 'any' : 'any';
      next.push({
        field: first.field,
        tokens: first.tokens.map((item, index) => (index === slot ? { ...item, wild, unit: '' } : item)),
        rows: bucket.flatMap((group) => group.rows).sort((left, right) => left - right),
      });
    }

    current = next;
  }

  return current;
}

function summarize(group: Group, rows: readonly LogEntry[], read: number): LogPattern {
  const levels = new Map<string, number>();
  const sources = new Map<string, number>();
  let tone: LogLevelTone = 'quiet';

  for (const index of group.rows) {
    const log = rows[index]!;
    const level = logLevelLabel(log.raw, log.level);
    const source = logSourceLabel(log.raw);

    levels.set(level, (levels.get(level) ?? 0) + 1);
    if (source !== '—') sources.set(source, (sources.get(source) ?? 0) + 1);
    if (TONE_RANK[logLevelTone(level)] > TONE_RANK[tone]) tone = logLevelTone(level);
  }

  const mostFirst = (entries: Map<string, number>) =>
    [...entries.entries()].sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]));

  return {
    key: patternKey(group.tokens),
    tokens: group.tokens,
    count: group.rows.length,
    share: read > 0 ? group.rows.length / read : 0,
    tone,
    levels: mostFirst(levels).map(([level, count]) => ({ level, count })),
    sources: mostFirst(sources).map(([name, count]) => ({ name, count })),
    rows: group.rows,
    fragment: longestFixedRun(group.tokens),
    field: group.field,
  };
}

/** The longest stretch of the pattern with no changing place in it, as the logs write it. */
export function longestFixedRun(tokens: readonly PatternToken[]): string {
  let best = '';
  let run: string[] = [];
  const close = () => {
    const text = run.join(' ');
    if (text.length > best.length) best = text;
    run = [];
  };

  for (const token of tokens) {
    if (token.wild || token.text === '…') {
      // What stands right before and right after a changing place is still said by every log.
      if (token.lead) run.push(token.lead);
      close();
      if (token.unit || token.trail) run.push(`${token.unit}${token.trail}`);
      continue;
    }

    run.push(`${token.lead}${token.text}${token.trail}`);
  }
  close();

  const text = best.trim();

  return text.length >= MIN_FRAGMENT_LENGTH ? text : '';
}
