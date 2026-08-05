import {
  type I18nTree,
  SOURCE_LOCALE,
  extractPlaceholders,
  listLocales,
  listNamespaces,
  readNamespace,
  setsEqual,
  writeNamespace,
} from './shared';

/**
 * Fills in missing translations for every non-source locale, taking the source
 * locale (en-us) as the authority. Only absent keys are translated — existing
 * translations are preserved — so this is safe to re-run as copy grows.
 *
 * The translation engine is a pluggable LLM provider (Claude / OpenAI / DeepSeek),
 * selected with --provider or TRANSLATE_PROVIDER (default: claude). Each provider
 * reads its own API key from the environment and its own default model, both
 * overridable. The provider SDK is imported lazily, so only the selected one
 * needs to be reachable.
 *
 * Usage:
 *   pnpm i18n:translate                         # all target locales, missing keys
 *   pnpm i18n:translate --locale=zh-cn          # one locale (may be brand-new)
 *   pnpm i18n:translate --namespace=alerts      # one namespace
 *   pnpm i18n:translate --all                   # re-translate every key, not just missing
 *   pnpm i18n:translate --dry-run               # list what would be translated, call no API
 *   pnpm i18n:translate --provider=deepseek --model=deepseek-V4-Pro
 */

type ProviderId = 'claude' | 'openai' | 'deepseek';

interface LlmProvider {
  id: ProviderId;
  model: string;
  complete(system: string, user: string): Promise<string>;
}

interface TranslatableEntry {
  /** Dotted path used as the JSON key in the request/response. */
  key: string;
  /** Source-language text. */
  text: string;
}

const BATCH_SIZE = 40;
const MAX_TOKENS = 16000;

const ENV_KEYS: Record<ProviderId, string> = {
  claude: 'ANTHROPIC_API_KEY',
  openai: 'OPENAI_API_KEY',
  deepseek: 'DEEPSEEK_API_KEY',
};

const DEFAULT_MODELS: Record<ProviderId, string> = {
  claude: 'claude-opus-4-8',
  openai: 'gpt-4o',
  deepseek: 'deepseek-V4-Pro',
};

const DEEPSEEK_BASE_URL = 'https://api.deepseek.com';

/** Human-readable language names handed to the model. Extend as locales are added. */
const LANGUAGE_NAMES: Record<string, string> = {
  'en-us': 'English (United States)',
  'zh-cn': 'Simplified Chinese (zh-CN)',
};

function argValue(flag: string): string | undefined {
  const prefix = `--${flag}=`;
  return process.argv.find((arg) => arg.startsWith(prefix))?.slice(prefix.length);
}

function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

function chunk<T>(items: T[], size: number): T[][] {
  const batches: T[][] = [];
  for (let index = 0; index < items.length; index += size) {
    batches.push(items.slice(index, index + size));
  }
  return batches;
}

function languageName(locale: string): string {
  return LANGUAGE_NAMES[locale] ?? locale;
}

/** Collect every string the target is missing (or every string, when `all`). */
function collectMissing(source: I18nTree, existing: I18nTree, all: boolean, prefix = ''): TranslatableEntry[] {
  const entries: TranslatableEntry[] = [];
  for (const [key, value] of Object.entries(source)) {
    const path = prefix ? `${prefix}.${key}` : key;
    const current = existing[key];
    if (typeof value === 'string') {
      if (all || typeof current !== 'string') entries.push({ key: path, text: value });
    } else {
      const child = typeof current === 'object' && current !== null ? current : {};
      entries.push(...collectMissing(value, child, all, path));
    }
  }
  return entries;
}

/** Rebuild the target in the source's key order, filling translations and keeping orphans. */
function rebuild(source: I18nTree, existing: I18nTree, translations: Map<string, string>, prefix = ''): I18nTree {
  const out: I18nTree = {};
  for (const [key, value] of Object.entries(source)) {
    const path = prefix ? `${prefix}.${key}` : key;
    const current = existing[key];
    if (typeof value === 'string') {
      const translated = translations.get(path);
      out[key] = translated ?? (typeof current === 'string' ? current : value);
    } else {
      const child = typeof current === 'object' && current !== null ? current : {};
      out[key] = rebuild(value, child, translations, path);
    }
  }
  // Keep keys the target has but the source dropped (orphans) rather than deleting them.
  for (const [key, value] of Object.entries(existing)) {
    if (!(key in source)) out[key] = value;
  }
  return out;
}

function buildSystemPrompt(targetLocale: string): string {
  return [
    `You are a professional software-localization translator. Translate UI strings for a web application from ${languageName(SOURCE_LOCALE)} to ${languageName(targetLocale)}.`,
    '',
    'Rules:',
    '- Return ONLY a JSON object mapping each input key to its translated string. No prose, no markdown, no code fences.',
    '- Preserve every {{placeholder}} token exactly as written — same name, same double braces. Never translate, rename, or reorder them.',
    '- Keep literal text inside single braces such as {field} unchanged.',
    '- Do not translate the product name "MoleSignal", code identifiers, HTML tags, or technical tokens (URLs, MIME types, header names).',
    '- Keep translations concise and natural for buttons, labels, and short UI copy.',
    '- Preserve leading/trailing whitespace and the ellipsis character (…) where present.',
  ].join('\n');
}

function parseJsonObject(raw: string): Record<string, unknown> {
  let text = raw.trim();
  const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/);
  if (fenced?.[1]) text = fenced[1].trim();
  const first = text.indexOf('{');
  const last = text.lastIndexOf('}');
  if (first !== -1 && last > first) text = text.slice(first, last + 1);
  const parsed = JSON.parse(text) as unknown;
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    throw new Error('model did not return a JSON object');
  }
  return parsed as Record<string, unknown>;
}

async function makeProvider(id: ProviderId, model: string): Promise<LlmProvider> {
  const apiKey = process.env[ENV_KEYS[id]];
  if (!apiKey) fail(`Set ${ENV_KEYS[id]} to use the ${id} provider.`);

  if (id === 'claude') {
    const { default: Anthropic } = await import('@anthropic-ai/sdk');
    const client = new Anthropic({ apiKey });
    return {
      id,
      model,
      // No temperature: Opus 4.x rejects sampling params. Thinking is left off
      // for this short, deterministic task; the system prompt pins JSON output.
      async complete(system, user) {
        const message = await client.messages.create({
          model,
          max_tokens: MAX_TOKENS,
          system,
          messages: [{ role: 'user', content: user }],
        });
        return message.content.map((block) => (block.type === 'text' ? block.text : '')).join('');
      },
    };
  }

  const { default: OpenAI } = await import('openai');
  const client = new OpenAI({ apiKey, ...(id === 'deepseek' ? { baseURL: DEEPSEEK_BASE_URL } : {}) });
  return {
    id,
    model,
    async complete(system, user) {
      const response = await client.chat.completions.create({
        model,
        temperature: 0,
        response_format: { type: 'json_object' },
        messages: [
          { role: 'system', content: system },
          { role: 'user', content: user },
        ],
      });
      return response.choices[0]?.message?.content ?? '';
    },
  };
}

interface NamespacePlan {
  locale: string;
  namespace: string;
  source: I18nTree;
  existing: I18nTree;
  entries: TranslatableEntry[];
}

async function translateNamespace(provider: LlmProvider, plan: NamespacePlan): Promise<{ filled: number; skipped: number }> {
  const system = buildSystemPrompt(plan.locale);
  const translations = new Map<string, string>();
  let skipped = 0;

  for (const batch of chunk(plan.entries, BATCH_SIZE)) {
    const request = Object.fromEntries(batch.map((entry) => [entry.key, entry.text]));
    let answer: Record<string, unknown>;
    try {
      answer = parseJsonObject(await provider.complete(system, JSON.stringify(request, null, 2)));
    } catch (error) {
      console.error(`  ! ${plan.locale}/${plan.namespace}: batch failed (${(error as Error).message}); left untranslated`);
      skipped += batch.length;
      continue;
    }

    for (const entry of batch) {
      const value = answer[entry.key];
      if (typeof value !== 'string') {
        skipped += 1;
        continue;
      }
      // Drop translations that mangled an interpolation slot; leave the key for a retry.
      if (!setsEqual(extractPlaceholders(entry.text), extractPlaceholders(value))) {
        console.error(`  ! ${plan.locale}/${plan.namespace}: placeholder mismatch on "${entry.key}"; left untranslated`);
        skipped += 1;
        continue;
      }
      translations.set(entry.key, value);
    }
  }

  if (translations.size > 0) {
    writeNamespace(plan.locale, plan.namespace, rebuild(plan.source, plan.existing, translations));
  }
  return { filled: translations.size, skipped };
}

function resolveProviderId(): ProviderId {
  const raw = argValue('provider') ?? process.env.TRANSLATE_PROVIDER ?? 'claude';
  if (raw !== 'claude' && raw !== 'openai' && raw !== 'deepseek') {
    fail(`Unknown provider "${raw}". Use one of: claude, openai, deepseek.`);
  }
  return raw;
}

function readExisting(locale: string, namespace: string): I18nTree {
  return listNamespaces(locale).includes(namespace) ? readNamespace(locale, namespace) : {};
}

async function main(): Promise<void> {
  const dryRun = process.argv.includes('--dry-run');
  const all = process.argv.includes('--all');
  const localeArg = argValue('locale');
  const namespaceFilter = argValue('namespace');

  if (localeArg === SOURCE_LOCALE) fail(`${SOURCE_LOCALE} is the source locale; nothing to translate into it.`);

  const targets = localeArg ? [localeArg] : listLocales().filter((locale) => locale !== SOURCE_LOCALE);
  const namespaces = listNamespaces(SOURCE_LOCALE).filter((ns) => !namespaceFilter || ns === namespaceFilter);

  const plans: NamespacePlan[] = [];
  for (const locale of targets) {
    for (const namespace of namespaces) {
      const source = readNamespace(SOURCE_LOCALE, namespace);
      const existing = readExisting(locale, namespace);
      const entries = collectMissing(source, existing, all);
      if (entries.length > 0) plans.push({ locale, namespace, source, existing, entries });
    }
  }

  const totalEntries = plans.reduce((sum, plan) => sum + plan.entries.length, 0);
  if (totalEntries === 0) {
    console.log(`translate-i18n: nothing to translate${all ? '' : ' — every target locale already covers the source'}.`);
    return;
  }

  if (dryRun) {
    for (const plan of plans) {
      console.log(`${plan.locale}/${plan.namespace}: ${plan.entries.length} string(s)`);
      for (const entry of plan.entries) console.log(`  ${entry.key}`);
    }
    console.log(`\ntranslate-i18n: ${totalEntries} string(s) across ${plans.length} namespace(s) would be translated (dry run).`);
    return;
  }

  const providerId = resolveProviderId();
  const model = argValue('model') ?? process.env.TRANSLATE_MODEL ?? DEFAULT_MODELS[providerId];
  console.log(`translate-i18n: ${providerId} (${model}) → ${totalEntries} string(s) across ${plans.length} namespace(s)`);

  const provider = await makeProvider(providerId, model);
  let filledTotal = 0;
  let skippedTotal = 0;
  for (const plan of plans) {
    const { filled, skipped } = await translateNamespace(provider, plan);
    filledTotal += filled;
    skippedTotal += skipped;
    console.log(`  ${plan.locale}/${plan.namespace}: +${filled}${skipped ? ` (${skipped} left)` : ''}`);
  }

  console.log(`\ntranslate-i18n: filled ${filledTotal} string(s)${skippedTotal ? `, ${skippedTotal} left untranslated` : ''}. Run \`pnpm i18n:check\` to verify.`);
  if (skippedTotal > 0) process.exit(1);
}

main().catch((error: unknown) => fail(`translate-i18n: ${(error as Error).message}`));
