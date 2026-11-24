import { SOURCE_LOCALE } from './shared';

/**
 * The translation engine of `backend.ts`, which fills the text the server writes: the model
 * providers, how each target language is described to the model, and a request pool. The web
 * interface is translated by i18n-cli instead (`i18n.config.json`).
 */

export type ProviderId = 'claude' | 'openai' | 'deepseek';

export interface LlmProvider {
  id: ProviderId;
  model: string;
  complete(system: string, user: string): Promise<string>;
}

const ENV_KEYS: Record<ProviderId, string> = {
  claude: 'ANTHROPIC_API_KEY',
  openai: 'OPENAI_API_KEY',
  deepseek: 'DEEPSEEK_API_KEY',
};

const DEFAULT_MODELS: Record<ProviderId, string> = {
  claude: 'claude-opus-5-5',
  openai: 'gpt-4o',
  deepseek: 'deepseek-V4-Pro',
};

const DEEPSEEK_BASE_URL = 'https://api.deepseek.com';
const MAX_TOKENS = 16000;
const ATTEMPTS = 3;

/** Language names handed to the model, with the register each market expects. */
export const LANGUAGES: Record<string, { name: string; style: string }> = {
  'en-us': { name: 'English (United States)', style: '' },
  'zh-cn': { name: 'Simplified Chinese (zh-CN)', style: '' },
  'zh-tw': {
    name: 'Traditional Chinese as written in Taiwan (zh-TW)',
    style:
      'Use Taiwan software vocabulary, not a character-by-character conversion: 伺服器, 資料, 程式, 檔案, 設定, 介面, 預設, 網路, 訊息, 使用者, 登入, 帳號.',
  },
  'ja-jp': {
    name: 'Japanese (ja-JP)',
    style:
      'Use the polite です/ます style for sentences and plain noun phrases for labels and buttons, as Japanese SaaS consoles do. Write established technical terms in katakana (ダッシュボード, アラート, トレース).',
  },
  'ko-kr': {
    name: 'Korean (ko-KR)',
    style:
      'Use the formal 합니다 style for sentences and short noun phrases for labels and buttons, as Korean SaaS consoles do.',
  },
  de: {
    name: 'German (de)',
    style: 'Address the reader with "Sie". Keep labels short; prefer established German IT terms but keep English ones Germans use (Dashboard, Trace, Span, Log).',
  },
  es: {
    name: 'Spanish (es), neutral international Spanish',
    style: 'Address the reader with "tú", as current Spanish software does; avoid words particular to Spain or to any single Latin American country.',
  },
  fr: {
    name: 'French (fr)',
    style: 'Address the reader with "vous". Use French typography: a non-breaking space before : ; ? ! and inside « ».',
  },
  'pt-br': {
    name: 'Brazilian Portuguese (pt-BR)',
    style: 'Address the reader with "você". Use Brazilian vocabulary (arquivo, tela, usuário), not European Portuguese.',
  },
};

export function argValue(flag: string): string | undefined {
  const prefix = `--${flag}=`;
  return process.argv.find((arg) => arg.startsWith(prefix))?.slice(prefix.length);
}

export function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

export function languageName(locale: string): string {
  return LANGUAGES[locale]?.name ?? locale;
}

export function buildSystemPrompt(targetLocale: string, fromLocale: string): string {
  const style = LANGUAGES[targetLocale]?.style;
  const conversion =
    fromLocale === SOURCE_LOCALE
      ? []
      : [`- The input is already a reviewed translation; keep its meaning and wording choices, and adapt it to ${languageName(targetLocale)}.`];
  const what =
    'text the server writes: emails, chat-app and notification messages, and the names and descriptions of built-in items';
  const where =
    '- The key path tells you where the text is sent (an email subject, a chat message, a built-in name); use it as context, never translate it.';
  const placeholders = [
    '- Preserve every {placeholder} token exactly as written — same name, same braces. Never translate or rename them; reorder them only when the grammar needs it.',
    '- Keep line breaks (\\n), Markdown and chat-app markup (*bold*, <url|label>, links) as they are, translating only the words.',
    '- Keep template expressions such as {{message.title}} and {{#if …}}…{{/if}} exactly as written.',
  ];

  return [
    `You are a professional software-localization translator. Translate ${what} of MoleSignal, an open-source observability and incident-response product (logs, metrics, traces, RUM, alerting, on-call, status pages, an AI agent), from ${languageName(fromLocale)} to ${languageName(targetLocale)}.`,
    '',
    'Rules:',
    '- Return ONLY a JSON object mapping each input key to its translated string. No prose, no markdown, no code fences.',
    where,
    ...placeholders,
    '- Keep <0>…</0> style tags and HTML tags exactly, translating only the text between them.',
    '- Do not translate the product name "MoleSignal", "Mole Agent", other product and vendor names, code identifiers, query languages (SQL, PromQL), units, or technical tokens (URLs, MIME types, header names, field names in backticks).',
    '- Use the terminology established in localized observability products (Datadog, Grafana, New Relic) for this language, and translate the same English term the same way every time.',
    '- Keep translations concise and natural; keep sentence case where the language has case.',
    '- Preserve leading/trailing whitespace and the ellipsis character (…) where present.',
    ...conversion,
    ...(style ? [`- ${style}`] : []),
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
      async complete(system, user) {
        const message = await client.beta.messages.create({
          model,
          max_tokens: MAX_TOKENS,
          // Translation is a short, well-specified task: low effort keeps cost and time down.
          output_config: { effort: 'low' },
          // A declined batch is re-run on another model inside the same call.
          betas: ['server-side-fallback-2026-07-01'],
          fallbacks: 'default',
          system,
          messages: [{ role: 'user', content: user }],
        });
        if (message.stop_reason === 'refusal') throw new Error('the model declined this batch');
        if (message.stop_reason === 'max_tokens') throw new Error('the answer was cut off');
        return message.content.map((block) => (block.type === 'text' ? block.text : '')).join('');
      },
    };
  }

  const { default: OpenAI } = await import('openai');
  const baseURL = id === 'deepseek' ? DEEPSEEK_BASE_URL : process.env.OPENAI_BASE_URL || undefined;
  const client = new OpenAI({ apiKey, baseURL });
  return {
    id,
    model,
    async complete(system, user) {
      const response = await client.chat.completions.create({
        model,
        // Reasoning models refuse a temperature other than the default; only older chat
        // models take 0, which keeps their wording steady from batch to batch.
        ...(/^gpt-4/.test(model) ? { temperature: 0 } : {}),
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

/** The provider and model `--provider` / `--model` (or their env vars) choose. */
export async function connectProvider(): Promise<LlmProvider> {
  // An unset GitHub Actions variable arrives as an empty string, which means the default here.
  const raw = argValue('provider') || process.env.TRANSLATE_PROVIDER || 'claude';
  if (raw !== 'claude' && raw !== 'openai' && raw !== 'deepseek') {
    fail(`Unknown provider "${raw}". Use one of: claude, openai, deepseek.`);
  }
  const model = argValue('model') || process.env.TRANSLATE_MODEL || DEFAULT_MODELS[raw];
  return makeProvider(raw, model);
}

/** Asks for one batch of `{ key: text }`, trying again when the answer cannot be read. */
export async function requestTranslations(
  provider: LlmProvider,
  system: string,
  texts: Record<string, string>,
): Promise<Record<string, unknown>> {
  const request = JSON.stringify(texts, null, 2);
  let lastError: unknown;

  for (let attempt = 1; attempt <= ATTEMPTS; attempt += 1) {
    try {
      return parseJsonObject(await provider.complete(system, request));
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => setTimeout(resolve, 2_000 * attempt));
    }
  }
  throw lastError;
}

/** Runs the jobs with at most `limit` of them in flight. */
export async function runPool(jobs: Array<() => Promise<void>>, limit: number): Promise<void> {
  let next = 0;
  const worker = async () => {
    while (next < jobs.length) {
      const job = jobs[next];
      next += 1;
      if (job) await job();
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, jobs.length) }, worker));
}

export function chunk<T>(items: T[], size: number): T[][] {
  const batches: T[][] = [];
  for (let index = 0; index < items.length; index += size) {
    batches.push(items.slice(index, index + size));
  }
  return batches;
}

/** The concurrency `--concurrency` asks for. */
export function concurrencyArg(fallback: number): number {
  const concurrency = Number(argValue('concurrency') ?? fallback);
  if (!Number.isInteger(concurrency) || concurrency < 1) fail('--concurrency takes a whole number of at least 1.');
  return concurrency;
}
