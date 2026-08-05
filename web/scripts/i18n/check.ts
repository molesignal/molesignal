import {
  SOURCE_LOCALE,
  extractPlaceholders,
  flatten,
  listLocales,
  listNamespaces,
  readNamespace,
  setsEqual,
} from './shared';

/**
 * Validates every non-source locale against the source locale (en-us):
 *   - missing      key exists in source, absent in target
 *   - orphan       key exists in target, absent in source (source dropped it)
 *   - placeholder  shared key whose {{slots}} differ between source and target
 *   - missing-file source namespace has no file in the target locale
 *   - extra-file   target has a namespace file the source locale doesn't
 *
 * Exits non-zero on any finding, so it can gate CI. Filter with
 * `--locale=zh-cn` / `--namespace=alerts`; emit machine output with `--json`.
 */

type Kind = 'missing' | 'orphan' | 'placeholder' | 'missing-file' | 'extra-file';

interface Finding {
  locale: string;
  namespace: string;
  kind: Kind;
  key: string;
  detail?: string;
}

function argValue(flag: string): string | undefined {
  const prefix = `--${flag}=`;
  return process.argv.find((arg) => arg.startsWith(prefix))?.slice(prefix.length);
}

const asJson = process.argv.includes('--json');
const localeFilter = argValue('locale');
const namespaceFilter = argValue('namespace');
const keep = (namespace: string) => !namespaceFilter || namespace === namespaceFilter;

const sourceNamespaces = listNamespaces(SOURCE_LOCALE);
const targets = listLocales().filter(
  (locale) => locale !== SOURCE_LOCALE && (!localeFilter || locale === localeFilter),
);

const findings: Finding[] = [];

for (const locale of targets) {
  const targetNamespaces = new Set(listNamespaces(locale));

  for (const namespace of sourceNamespaces) {
    if (!keep(namespace)) continue;

    if (!targetNamespaces.has(namespace)) {
      findings.push({ locale, namespace, kind: 'missing-file', key: '*' });
      continue;
    }

    const source = flatten(readNamespace(SOURCE_LOCALE, namespace));
    const target = flatten(readNamespace(locale, namespace));

    for (const key of Object.keys(source)) {
      if (!(key in target)) findings.push({ locale, namespace, kind: 'missing', key });
    }
    for (const key of Object.keys(target)) {
      if (!(key in source)) findings.push({ locale, namespace, kind: 'orphan', key });
    }
    for (const key of Object.keys(source)) {
      const sourceText = source[key];
      const targetText = target[key];
      if (sourceText === undefined || targetText === undefined) continue;

      const sourceSlots = extractPlaceholders(sourceText);
      const targetSlots = extractPlaceholders(targetText);
      if (!setsEqual(sourceSlots, targetSlots)) {
        findings.push({
          locale,
          namespace,
          kind: 'placeholder',
          key,
          detail: `source {{${[...sourceSlots].join(', ')}}} vs target {{${[...targetSlots].join(', ')}}}`,
        });
      }
    }
  }

  for (const namespace of targetNamespaces) {
    if (keep(namespace) && !sourceNamespaces.includes(namespace)) {
      findings.push({ locale, namespace, kind: 'extra-file', key: '*' });
    }
  }
}

if (asJson) {
  console.log(JSON.stringify({ ok: findings.length === 0, source: SOURCE_LOCALE, findings }, null, 2));
  process.exit(findings.length ? 1 : 0);
}

if (findings.length === 0) {
  console.log(
    `check-i18n: ${targets.length} locale(s) consistent with ${SOURCE_LOCALE} across ${sourceNamespaces.length} namespaces.`,
  );
  process.exit(0);
}

for (const { locale, namespace, kind, key, detail } of findings) {
  const where = `${locale}/${namespace}`;
  console.error(`${where}  ${kind}: ${key}${detail ? `  (${detail})` : ''}`);
}

const counts = findings.reduce<Record<Kind, number>>(
  (acc, finding) => {
    acc[finding.kind] += 1;
    return acc;
  },
  { missing: 0, orphan: 0, placeholder: 0, 'missing-file': 0, 'extra-file': 0 },
);

const summary = (Object.entries(counts) as [Kind, number][])
  .filter(([, count]) => count > 0)
  .map(([kind, count]) => `${count} ${kind}`)
  .join(', ');

console.error(`\ncheck-i18n: ${findings.length} issue(s) — ${summary}. Run \`pnpm i18n:translate\` to fill missing keys.`);
process.exit(1);
