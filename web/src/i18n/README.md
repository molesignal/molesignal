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

`en-us` is the source of truth, and `zh-cn` is written by hand beside it: a
change that adds or edits copy changes both (`WRITTEN_LOCALES` in `locales.ts`).
The other locales are machine-translated. When `en-us` or `zh-cn` changes on
`main`, the Translate workflow (`.github/workflows/translate.yml`) translates
what is new or whose source changed, for the interface and for the text the
server writes, and commits the result to `main`. Until it has run, a new key
shows its English text in those locales.

## `pnpm i18n:check`

Validates the locales against `en-us` and exits non-zero on any problem. By
default it checks the locales written by hand, as `pnpm lint` does; `--strict`
checks every locale, as the Translate workflow does after translating. It
reports:

| Finding        | Meaning                                                       |
| -------------- | ------------------------------------------------------------- |
| `missing`      | key exists in `en-us`, absent in the target locale            |
| `orphan`       | key exists in the target, absent in `en-us` (source dropped it) |
| `placeholder`  | a shared key whose `{{slots}}` differ between source and target |
| `missing-file` | an `en-us` namespace has no file in the target locale          |
| `extra-file`   | the target has a namespace file `en-us` doesn't               |

```bash
pnpm i18n:check                      # the locales written by hand
pnpm i18n:check --strict             # every locale
pnpm i18n:check --locale=ja-jp       # one locale
pnpm i18n:check --namespace=alerts   # one namespace
pnpm i18n:check --json               # machine-readable output
```

It is dependency-free and makes no network calls — safe to run anywhere.

## `pnpm i18n:translate`

Runs [i18n-cli](https://github.com/molesignal/i18n-cli) twice:
`i18n.config.json` translates `en-us` into `ja-jp`, `ko-kr`, `de`, `es`, `fr`
and `pt-br`, and `i18n.zh-tw.config.json` writes `zh-tw` from `zh-cn`.
`scripts/i18n/instructions.md` holds the guidance the model gets, with the
register of each language. CI runs it; to run it locally, install the CLI and
set the key and model:

```bash
go install github.com/molesignal/i18n-cli/cmd/i18n-cli@v0.3.0
export OPENAI_API_KEY=... OPENAI_MODEL=...
pnpm i18n:translate
```

- **Incremental.** `.i18n-cli-state.json` records the source text each string
  was translated from, so a string is translated again when its source changes,
  and only then. `--adopt-existing` keeps a translation that has no record yet.
- **Plural forms.** French, Spanish and Portuguese have a `many` plural form
  ("1 million") that English lacks: with `"plurals": "i18next"` each locale gets
  the CLDR forms of its language, translated from `key_other`.
- **Placeholders and tags are checked.** A translation that changes a
  `{{slot}}`, a printf directive or an HTML tag is rejected and asked for again.
- Keys keep the source order, and a key the source drops is removed.

## `pnpm i18n:translate-backend`

Translates the `i18n_language` rows of the baseline migration, the text the
server writes, from `en-us` (`zh-tw` from `zh-cn`). It adds the rows a language
lacks and translates again the rows whose source changed, which
`scripts/i18n/backend.state.json` records. It uses `TRANSLATE_PROVIDER`
(`claude`, `openai` or `deepseek`) with that provider's API key and
`TRANSLATE_MODEL`; `--dry-run` counts what it would translate.

## Adding a locale

1. Register it in `SUPPORTED_LOCALES`, `LOCALE_LABELS`, `matchLocale` and the
   Intl tag and docs directory tables in `src/i18n/locales.ts`; add
   `bundles/<new>.ts` and its loader in `index.ts`; add a date-fns locale in
   `shell/ui/calendar.tsx`; add it to the targets of `i18n.config.json`, its
   register to `scripts/i18n/instructions.md`, and its name and style to
   `LANGUAGES` in `scripts/i18n/engine.ts`. The backend lists its languages too
   (`crates/core/domain/src/domain/i18n_language.rs`), and so does the docs site
   (`../docs/scripts/i18n/languages.mjs`).
2. Push to `main`, or run the Translate workflow by hand: it creates the
   directory and every namespace file from `en-us`.
3. Review the generated copy; machine translation is a first pass, not a final one.
