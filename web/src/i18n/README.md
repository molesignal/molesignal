# Translations

UI copy lives here as one JSON file per namespace, per locale:

```
src/i18n/
  locales.ts  ← SUPPORTED_LOCALES, their names and Intl/docs mappings
  index.ts    ← i18next set-up: en-us bundled, every other locale loaded on demand
  bundles/    ← one module per locale, so each language is one lazy chunk
  en-us/      ← source locale (DEFAULT_LOCALE), the authority for keys
  zh-cn/ zh-tw/ ja-jp/ ko-kr/ de/ es/ fr/ pt-br/
```

Only `en-us` ships in the main bundle; it is the fallback for any key a
locale lacks. `loadLocale(language)` fetches a locale's chunk, and `main.tsx`
awaits the chosen one before the first render. A component that reads copy in
a language other than the interface's (a public status page) uses
`fixedT(language, namespace)`, which loads that language and returns its `t`.

Code that formats by language goes through `locales.ts` (`intlLocale`,
`docsLocale`, `isChinese`, `isMainlandChinese`), never `startsWith('zh')`:
`zh-tw` is Chinese too, but not Simplified Chinese.

`en-us` is the source of truth. Every other locale is expected to mirror its
key set. Two scripts keep that true: `i18n:check` verifies it, `i18n:translate`
fills the gaps.

## `pnpm i18n:check`

Validates every non-source locale against `en-us` and exits non-zero on any
problem, so it can gate CI or a pre-commit hook. It reports:

| Finding        | Meaning                                                       |
| -------------- | ------------------------------------------------------------- |
| `missing`      | key exists in `en-us`, absent in the target locale            |
| `orphan`       | key exists in the target, absent in `en-us` (source dropped it) |
| `placeholder`  | a shared key whose `{{slots}}` differ between source and target |
| `missing-file` | an `en-us` namespace has no file in the target locale          |
| `extra-file`   | the target has a namespace file `en-us` doesn't               |

```bash
pnpm i18n:check                      # all locales, all namespaces
pnpm i18n:check --locale=zh-cn       # one locale
pnpm i18n:check --namespace=alerts   # one namespace
pnpm i18n:check --json               # machine-readable output
```

It is dependency-free and makes no network calls — safe to run anywhere.

## `pnpm i18n:translate`

Translates the keys a target locale is **missing**, taking `en-us` as the
source. Existing translations are preserved, so it is safe to re-run as copy
grows. Output is written back in the repo's canonical shape (2-space indent,
trailing newline, source key order); orphan keys are kept, not deleted.

```bash
pnpm i18n:translate                       # all target locales, missing keys only
pnpm i18n:translate --locale=zh-cn        # one locale (a brand-new dir is created if needed)
pnpm i18n:translate --namespace=alerts    # one namespace
pnpm i18n:translate --all                 # re-translate every key, not just missing ones
pnpm i18n:translate --concurrency=8       # requests in flight at once (default 6)
pnpm i18n:translate --dry-run             # list what would be translated, call no API
```

`zh-tw` is translated from `zh-cn`, not from English, so run it after
`zh-cn` is complete. Each namespace is written after every batch, and a key
whose batch failed is left out rather than filled with English, so running
the same command again picks up exactly what is missing.

French, Spanish and Portuguese have a `many` plural form ("1 million") that
English lacks; the script writes `key_many` as a copy of `key_other`, and
`i18n:check` accepts it.

`--dry-run` is free — use it to preview scope before spending tokens.

### Translation engine

The engine is a pluggable LLM provider. Pick one with `--provider` or the
`TRANSLATE_PROVIDER` env var (default: `claude`). Each reads its own API key
from the environment; the provider SDK is imported lazily, so only the selected
one needs to be installed/reachable.

| Provider   | API key env var      | Default model     |
| ---------- | -------------------- | ----------------- |
| `claude`   | `ANTHROPIC_API_KEY`  | `claude-opus-5-5` |
| `openai`   | `OPENAI_API_KEY`     | `gpt-4o`          |
| `deepseek` | `DEEPSEEK_API_KEY`   | `deepseek-V4-Pro` |

Override the model with `--model=...` or `TRANSLATE_MODEL`. Examples:

```bash
ANTHROPIC_API_KEY=sk-... pnpm i18n:translate --locale=zh-cn
TRANSLATE_PROVIDER=deepseek DEEPSEEK_API_KEY=sk-... pnpm i18n:translate
pnpm i18n:translate --provider=claude --model=claude-sonnet-5-5   # cheaper model
```

### Safeguards

- **Placeholders are protected.** `{{name}}` interpolation slots must survive
  translation unchanged. A translation that adds, drops, or renames a slot is
  rejected and the key is left untranslated for a retry (and flagged to stderr).
- **Incremental by default.** Only missing keys are sent to the model; the rest
  of the file is untouched.
- **Verify after.** The run ends by pointing you at `pnpm i18n:check`. Wire that
  into CI so a missed key fails the build instead of shipping a raw key to users.

## Adding a locale

1. Register it in `SUPPORTED_LOCALES`, `LOCALE_LABELS`, `matchLocale` and the
   Intl tag table in `src/i18n/locales.ts`; add `bundles/<new>.ts` and its
   loader in `index.ts`; add a date-fns locale in `shell/ui/calendar.tsx`; add
   its name and style to `LANGUAGES` in `scripts/i18n/translate.ts`. The
   backend lists its languages too (`crates/core/domain/src/domain/i18n_language.rs`).
2. Run `pnpm i18n:translate --locale=<new>` — the directory and all namespace
   files are created from `en-us`.
3. Run `pnpm i18n:check --locale=<new>` to confirm full coverage.
4. Review the generated copy; machine translation is a first pass, not a final one.
