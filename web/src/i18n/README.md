# Translations

UI copy lives here as one JSON file per namespace, per locale:

```
src/i18n/
  en-us/      ← source locale (DEFAULT_LOCALE), the authority for keys
  zh-cn/
  <new>/      ← add a locale by translating en-us into a new dir
```

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
pnpm i18n:translate --dry-run             # list what would be translated, call no API
```

`--dry-run` is free — use it to preview scope before spending tokens.

### Translation engine

The engine is a pluggable LLM provider. Pick one with `--provider` or the
`TRANSLATE_PROVIDER` env var (default: `claude`). Each reads its own API key
from the environment; the provider SDK is imported lazily, so only the selected
one needs to be installed/reachable.

| Provider   | API key env var      | Default model     |
| ---------- | -------------------- | ----------------- |
| `claude`   | `ANTHROPIC_API_KEY`  | `claude-opus-4-8` |
| `openai`   | `OPENAI_API_KEY`     | `gpt-4o`          |
| `deepseek` | `DEEPSEEK_API_KEY`   | `deepseek-V4-Pro` |

Override the model with `--model=...` or `TRANSLATE_MODEL`. Examples:

```bash
ANTHROPIC_API_KEY=sk-... pnpm i18n:translate --locale=zh-cn
TRANSLATE_PROVIDER=deepseek DEEPSEEK_API_KEY=sk-... pnpm i18n:translate
pnpm i18n:translate --provider=claude --model=claude-haiku-4-5   # cheaper model
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

1. Register it in `SUPPORTED_LOCALES` (and `detectBrowserLocale`, the resource
   map, etc.) in `src/i18n/index.ts`.
2. Run `pnpm i18n:translate --locale=<new>` — the directory and all namespace
   files are created from `en-us`.
3. Run `pnpm i18n:check --locale=<new>` to confirm full coverage.
4. Review the generated copy; machine translation is a first pass, not a final one.
