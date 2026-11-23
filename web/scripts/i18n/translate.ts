import { SUPPORTED_LOCALES } from '../../src/i18n/locales';
import {
  type I18nTree,
  SOURCE_LOCALE,
  extractPlaceholders,
  flatten,
  listLocales,
  listNamespaces,
  pluralForms,
  readNamespace,
  setsEqual,
  writeNamespace,
} from './shared';
import {
  argValue,
  buildSystemPrompt,
  chunk,
  concurrencyArg,
  connectProvider,
  fail,
  type LlmProvider,
  requestTranslations,
  runPool,
} from './engine';

/**
 * Fills in missing translations for every non-source locale, taking the source
 * locale (en-us) as the authority for keys. Only absent keys are translated —
 * existing translations are preserved — so this is safe to re-run as copy
 * grows, or after a run that stopped part way.
 *
 * The translation engine is a pluggable LLM provider (Claude / OpenAI / DeepSeek),
 * selected with --provider or TRANSLATE_PROVIDER (default: claude). Each provider
 * reads its own API key from the environment and its own default model, both
 * overridable. The provider SDK is imported lazily, so only the selected one
 * needs to be reachable.
 *
 * Usage:
 *   pnpm i18n:translate                         # all target locales, missing keys
 *   pnpm i18n:translate --locale=ja-jp          # one locale (may be brand-new)
 *   pnpm i18n:translate --namespace=alerts      # one namespace
 *   pnpm i18n:translate --all                   # re-translate every key, not just missing
 *   pnpm i18n:translate --concurrency=8         # requests in flight at once (default 6)
 *   pnpm i18n:translate --dry-run               # list what would be translated, call no API
 *   pnpm i18n:translate --provider=deepseek --model=deepseek-V4-Pro
 */

interface TranslatableEntry {
  /** Dotted path used as the JSON key in the request/response. */
  key: string;
  /** Text to translate, in the language `translatedFrom` names. */
  text: string;
}

const BATCH_SIZE = 40;
const DEFAULT_CONCURRENCY = 6;

/** A target written from another translation rather than from English. */
const TRANSLATED_FROM: Record<string, string> = {
  'zh-tw': 'zh-cn',
};

/** Collect every string the target is missing (or every string, when `all`). */
function collectMissing(
  source: I18nTree,
  existing: I18nTree,
  reference: Record<string, string>,
  all: boolean,
  prefix = '',
): TranslatableEntry[] {
  const entries: TranslatableEntry[] = [];
  for (const [key, value] of Object.entries(source)) {
    const path = prefix ? `${prefix}.${key}` : key;
    const current = existing[key];
    if (typeof value === 'string') {
      if (all || typeof current !== 'string') entries.push({ key: path, text: reference[path] ?? value });
    } else {
      const child = typeof current === 'object' && current !== null ? current : {};
      entries.push(...collectMissing(value, child, reference, all, path));
    }
  }
  return entries;
}

/**
 * Rebuild the target in the source's key order, with the translations so far.
 * A key that has no translation yet is left out, so English shows for it and
 * the next run picks it up again. Orphans are kept.
 */
function rebuild(
  source: I18nTree,
  existing: I18nTree,
  translations: Map<string, string>,
  manyForm: boolean,
  prefix = '',
): I18nTree {
  const out: I18nTree = {};
  for (const [key, value] of Object.entries(source)) {
    const path = prefix ? `${prefix}.${key}` : key;
    const current = existing[key];
    if (typeof value === 'string') {
      const text = translations.get(path) ?? (typeof current === 'string' ? current : undefined);
      if (text !== undefined) out[key] = text;
    } else {
      const child = typeof current === 'object' && current !== null ? current : {};
      const built = rebuild(value, child, translations, manyForm, path);
      if (Object.keys(built).length > 0) out[key] = built;
    }
  }

  // French, Spanish and Portuguese have a `many` plural form ("1 million")
  // that English lacks; it reads as `other`.
  if (manyForm) {
    for (const key of Object.keys(source)) {
      if (!key.endsWith('_other')) continue;
      const many = `${key.slice(0, -'_other'.length)}_many`;
      const other = out[key];
      if (!(many in source) && typeof other === 'string') {
        out[many] = typeof existing[many] === 'string' ? existing[many] : other;
      }
    }
  }

  // Keep keys the target has but the source dropped (orphans) rather than deleting them.
  for (const [key, value] of Object.entries(existing)) {
    if (!(key in source) && !(key in out)) out[key] = value;
  }
  return out;
}

interface NamespacePlan {
  locale: string;
  fromLocale: string;
  namespace: string;
  source: I18nTree;
  existing: I18nTree;
  entries: TranslatableEntry[];
  translations: Map<string, string>;
  skipped: number;
  batchesLeft: number;
}

async function translateBatch(provider: LlmProvider, plan: NamespacePlan, batch: TranslatableEntry[]): Promise<void> {
  let answer: Record<string, unknown>;
  try {
    const system = buildSystemPrompt(plan.locale, plan.fromLocale, 'interface');
    answer = await requestTranslations(provider, system, Object.fromEntries(batch.map((entry) => [entry.key, entry.text])));
  } catch (error) {
    console.error(`  ! ${plan.locale}/${plan.namespace}: batch failed (${(error as Error).message}); left untranslated`);
    plan.skipped += batch.length;
    return;
  }

  for (const entry of batch) {
    const value = answer[entry.key];
    if (typeof value !== 'string' || value.trim() === '') {
      plan.skipped += 1;
      continue;
    }
    // Drop translations that mangled an interpolation slot; leave the key for a retry.
    if (!setsEqual(extractPlaceholders(entry.text), extractPlaceholders(value))) {
      console.error(`  ! ${plan.locale}/${plan.namespace}: placeholder mismatch on "${entry.key}"; left untranslated`);
      plan.skipped += 1;
      continue;
    }
    plan.translations.set(entry.key, value);
  }
}

/** Write what a namespace has so far, so a stopped run keeps its work. */
function save(plan: NamespacePlan): void {
  if (plan.translations.size === 0) return;
  const manyForm = pluralForms(plan.locale).has('many') && !pluralForms(SOURCE_LOCALE).has('many');
  writeNamespace(plan.locale, plan.namespace, rebuild(plan.source, plan.existing, plan.translations, manyForm));
}

function readExisting(locale: string, namespace: string): I18nTree {
  if (!listLocales().includes(locale)) return {};
  return listNamespaces(locale).includes(namespace) ? readNamespace(locale, namespace) : {};
}

async function main(): Promise<void> {
  const dryRun = process.argv.includes('--dry-run');
  const all = process.argv.includes('--all');
  const localeArg = argValue('locale');
  const namespaceFilter = argValue('namespace');
  const concurrency = concurrencyArg(DEFAULT_CONCURRENCY);

  if (localeArg === SOURCE_LOCALE) fail(`${SOURCE_LOCALE} is the source locale; nothing to translate into it.`);

  const known = [...new Set<string>([...SUPPORTED_LOCALES, ...listLocales()])];
  const targets = (localeArg ? [localeArg] : known.filter((locale) => locale !== SOURCE_LOCALE))
    // A locale translated from another comes after it.
    .sort((a, b) => Number(a in TRANSLATED_FROM) - Number(b in TRANSLATED_FROM));
  const namespaces = listNamespaces(SOURCE_LOCALE).filter((ns) => !namespaceFilter || ns === namespaceFilter);

  const plans: NamespacePlan[] = [];
  for (const locale of targets) {
    for (const namespace of namespaces) {
      const source = readNamespace(SOURCE_LOCALE, namespace);
      const existing = readExisting(locale, namespace);
      const fromLocale = TRANSLATED_FROM[locale] ?? SOURCE_LOCALE;
      const reference = fromLocale === SOURCE_LOCALE ? {} : flatten(readExisting(fromLocale, namespace));
      const entries = collectMissing(source, existing, reference, all);
      if (entries.length > 0) {
        plans.push({
          locale,
          fromLocale,
          namespace,
          source,
          existing,
          entries,
          translations: new Map(),
          skipped: 0,
          batchesLeft: 0,
        });
      }
    }
  }

  const totalEntries = plans.reduce((sum, plan) => sum + plan.entries.length, 0);
  if (totalEntries === 0) {
    console.log(`translate-i18n: nothing to translate${all ? '' : ' — every target locale already covers the source'}.`);
    return;
  }

  if (dryRun) {
    for (const plan of plans) {
      console.log(`${plan.locale}/${plan.namespace}: ${plan.entries.length} string(s) from ${plan.fromLocale}`);
    }
    console.log(`\ntranslate-i18n: ${totalEntries} string(s) across ${plans.length} namespace(s) would be translated (dry run).`);
    return;
  }

  const provider = await connectProvider();
  console.log(
    `translate-i18n: ${provider.id} (${provider.model}) → ${totalEntries} string(s) across ${plans.length} namespace(s), ${concurrency} at a time`,
  );

  const jobs: Array<() => Promise<void>> = [];
  for (const plan of plans) {
    const batches = chunk(plan.entries, BATCH_SIZE);
    plan.batchesLeft = batches.length;
    for (const batch of batches) {
      jobs.push(async () => {
        await translateBatch(provider, plan, batch);
        save(plan);
        plan.batchesLeft -= 1;
        if (plan.batchesLeft === 0) {
          console.log(
            `  ${plan.locale}/${plan.namespace}: +${plan.translations.size}${plan.skipped ? ` (${plan.skipped} left)` : ''}`,
          );
        }
      });
    }
  }
  await runPool(jobs, concurrency);

  const filledTotal = plans.reduce((sum, plan) => sum + plan.translations.size, 0);
  const skippedTotal = plans.reduce((sum, plan) => sum + plan.skipped, 0);
  console.log(
    `\ntranslate-i18n: filled ${filledTotal} string(s)${skippedTotal ? `, ${skippedTotal} left untranslated — run the same command again to retry them` : ''}. Run \`pnpm i18n:check\` to verify.`,
  );
  if (skippedTotal > 0) process.exit(1);
}

main().catch((error: unknown) => fail(`translate-i18n: ${(error as Error).message}`));
