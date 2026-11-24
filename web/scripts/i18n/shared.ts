import { mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** Web app root, resolved relative to this script (scripts/i18n/) so cwd doesn't matter. */
export const WEB = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

/** Directory holding one sub-directory of JSON resources per locale. */
export const I18N_DIR = join(WEB, 'src/i18n');

/**
 * Locale copy is authored in and translated from. Mirrors DEFAULT_LOCALE in
 * src/i18n/index.ts — keep the two in sync if the source language ever changes.
 */
export const SOURCE_LOCALE = 'en-us';

/** A namespace resource: nested objects bottoming out in translatable strings. */
export type I18nTree = { [key: string]: string | I18nTree };

const JSON_EXT = '.json';
const PLACEHOLDER_RE = /\{\{(.*?)\}\}/g;

/** Locale directories that actually hold namespace JSON, sorted. */
export function listLocales(): string[] {
  return readdirSync(I18N_DIR)
    .filter((entry) => {
      const dir = join(I18N_DIR, entry);
      return (
        statSync(dir).isDirectory() &&
        readdirSync(dir).some((file) => file.endsWith(JSON_EXT))
      );
    })
    .sort();
}

/** Namespace names (file stems) present for a locale, sorted. */
export function listNamespaces(locale: string): string[] {
  return readdirSync(join(I18N_DIR, locale))
    .filter((file) => file.endsWith(JSON_EXT))
    .map((file) => file.slice(0, -JSON_EXT.length))
    .sort();
}

function namespacePath(locale: string, namespace: string): string {
  return join(I18N_DIR, locale, `${namespace}${JSON_EXT}`);
}

export function readNamespace(locale: string, namespace: string): I18nTree {
  return JSON.parse(readFileSync(namespacePath(locale, namespace), 'utf8')) as I18nTree;
}

/** Write a namespace back in the repo's canonical shape: 2-space, trailing newline. */
export function writeNamespace(locale: string, namespace: string, tree: I18nTree): void {
  const path = namespacePath(locale, namespace);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(tree, null, 2)}\n`, 'utf8');
}

/** Flatten nested keys to dotted paths: { a: { b: 'x' } } -> { 'a.b': 'x' }. */
export function flatten(tree: I18nTree, prefix = ''): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(tree)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof value === 'string') {
      out[path] = value;
    } else {
      Object.assign(out, flatten(value, path));
    }
  }
  return out;
}

/** Names of the {{...}} interpolation slots in a string (order-independent). */
export function extractPlaceholders(text: string): Set<string> {
  const names = new Set<string>();
  for (const match of text.matchAll(PLACEHOLDER_RE)) {
    names.add((match[1] ?? '').trim());
  }
  return names;
}

export function setsEqual<T>(a: Set<T>, b: Set<T>): boolean {
  if (a.size !== b.size) return false;
  for (const value of a) {
    if (!b.has(value)) return false;
  }
  return true;
}

/**
 * The plural forms `Intl.PluralRules` gives a locale (`one`, `many`, `other`…).
 * i18next looks a counted key up as `key_<form>`.
 */
export function pluralForms(locale: string): Set<string> {
  return new Set(new Intl.PluralRules(locale).resolvedOptions().pluralCategories);
}

/**
 * A plural form the target language has and the source lacks, of a key the source counts (it
 * has `key_other`): French, Spanish and Portuguese say "1 million" with `_many`, and a key that
 * English writes only as `_other` still needs `_one` in German. i18n-cli adds these forms, so
 * they are part of the target, not orphans.
 */
export function pluralFormOf(locale: string, key: string, sourceKeys: ReadonlySet<string>): boolean {
  const match = /^(.+)_(zero|one|two|few|many|other)$/.exec(key);
  if (!match?.[1] || !match[2] || !pluralForms(locale).has(match[2] as Intl.LDMLPluralRule)) return false;

  return sourceKeys.has(`${match[1]}_other`);
}
