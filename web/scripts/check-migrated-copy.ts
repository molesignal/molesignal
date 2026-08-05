import { readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';

const WEB = process.cwd();

const TARGETS = [
  'src/routes/home/index.tsx',
  'src/routes/Datasource/Datasource.tsx',
  'src/routes/alerts/index.tsx',
  'src/routes/streams/index.tsx',
  'src/routes/reports/index.tsx',
  'src/routes/dashboards/index.tsx',
  'src/routes/dashboards/Editor.tsx',
  'src/routes/dashboards/Import.tsx',
  'src/routes/traces/Detail.tsx',
  'src/routes/traces/SessionDetail.tsx',
  'src/routes/traces/index.tsx',
  'src/routes/Investigate.tsx',
  'src/routes/ServiceGraph.tsx',
  'src/routes/placeholder.tsx',
  'src/routes/PublicShare.tsx',
  'src/routes/streams/Explore.tsx',
  'src/routes/logs/index.tsx',
  'src/routes/logs/Inspector.tsx',
  'src/routes/Metrics.tsx',
  'src/routes/Noc.tsx',
  'src/routes/functions',
  'src/routes/pipelines/List.tsx',
  'src/routes/pipelines/Add.tsx',
  'src/routes/pipelines/Backfill.tsx',
  'src/routes/pipelines/Edit.tsx',
  'src/routes/pipelines/History.tsx',
  'src/routes/pipelines/Import.tsx',
  'src/routes/pipelines/PipelineForm.tsx',
  'src/routes/settings',
  'src/routes/iam',
  'src/routes/rum',
] as const;

const USER_VISIBLE_ATTRIBUTES = new Set([
  'aria-label',
  'alt',
  'confirmLabel',
  'description',
  'emptyLabel',
  'header',
  'hint',
  'label',
  'loadingLabel',
  'placeholder',
  'submitLabel',
  'subtitle',
  'title',
]);

const USER_VISIBLE_OBJECT_KEYS = new Set([
  'confirmLabel',
  'description',
  'emptyDescription',
  'emptyLabel',
  'emptyTitle',
  'header',
  'hint',
  'label',
  'loadingLabel',
  'placeholder',
  'submitLabel',
  'subtitle',
  'title',
]);

const MACHINE_TOKEN = /^[a-z0-9_./:{}?=&%#@| -]+$/;
const ALLOWED_TEXT = new Set([
  'A',
  'Anthropic',
  'ID',
  'JSON',
  'MAP',
  'OTLP',
  'OpenAI',
  'OpenAI-compatible',
  'PromQL',
  'TLS',
  'VRL',
  'body',
  'config_json',
  'key_material_b64',
  'molesignal',
  'MoleSignal',
  'POST /license',
  'openid profile email',
]);

interface Violation {
  file: string;
  line: number;
  text: string;
  context: string;
}

function collectFiles(target: string): string[] {
  const abs = join(WEB, target);
  const stat = statSync(abs);
  if (stat.isFile()) return [abs];
  return readdirSync(abs)
    .flatMap((entry) => {
      const child = join(abs, entry);
      const childStat = statSync(child);
      if (childStat.isDirectory()) return collectFiles(relative(WEB, child));
      return child.endsWith('.tsx') ? [child] : [];
    })
    .filter((file) => file.endsWith('.tsx'));
}

function normalize(text: string) {
  return text.replace(/\s+/g, ' ').trim();
}

function containsHumanLanguage(text: string) {
  return /[A-Za-z\u4e00-\u9fff]/.test(text);
}

function isAllowedLiteral(text: string) {
  const normalized = normalize(text);
  if (!normalized) return true;
  if (!containsHumanLanguage(normalized)) return true;
  if (ALLOWED_TEXT.has(normalized)) return true;
  if (normalized.length <= 2) return true;
  if (normalized.includes('://')) return true;
  if (MACHINE_TOKEN.test(normalized) && !/\s/.test(normalized)) return true;
  if (/^\/?[a-z0-9_./:{}-]+$/.test(normalized)) return true;
  if (/^[A-Z0-9_./:{} -]+$/.test(normalized) && normalized.length <= 12) return true;
  return false;
}

function propName(name: ts.PropertyName): string | null {
  if (ts.isIdentifier(name) || ts.isStringLiteral(name) || ts.isNumericLiteral(name)) return name.text;
  return null;
}

function jsxAttrName(name: ts.JsxAttributeName): string {
  if (ts.isIdentifier(name)) return name.text;
  return name.getText();
}

function isTranslationCall(node: ts.Node): boolean {
  if (!ts.isCallExpression(node)) return false;
  const expr = node.expression;
  return ts.isIdentifier(expr) && (expr.text === 't' || expr.text === 'tc');
}

function insideTranslationCall(node: ts.Node): boolean {
  let current: ts.Node | undefined = node.parent;
  while (current) {
    if (isTranslationCall(current)) return true;
    current = current.parent;
  }
  return false;
}

function isToastCall(node: ts.CallExpression): boolean {
  const expr = node.expression;
  return (
    ts.isPropertyAccessExpression(expr) &&
    ts.isIdentifier(expr.expression) &&
    expr.expression.text === 'toast'
  );
}

function addViolation(
  violations: Violation[],
  source: ts.SourceFile,
  node: ts.Node,
  text: string,
  context: string,
) {
  const normalized = normalize(text);
  if (isAllowedLiteral(normalized)) return;
  const pos = source.getLineAndCharacterOfPosition(node.getStart(source));
  violations.push({
    file: relative(WEB, source.fileName),
    line: pos.line + 1,
    text: normalized,
    context,
  });
}

function auditFile(file: string): Violation[] {
  const program = ts.createSourceFile(file, ts.sys.readFile(file) ?? '', ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const violations: Violation[] = [];

  function visit(node: ts.Node) {
    if (ts.isJsxText(node)) {
      addViolation(violations, program, node, node.getText(program), 'JSX text');
    }

    if (ts.isJsxAttribute(node) && node.initializer && USER_VISIBLE_ATTRIBUTES.has(jsxAttrName(node.name))) {
      if (ts.isStringLiteral(node.initializer)) {
        addViolation(violations, program, node.initializer, node.initializer.text, `JSX prop ${jsxAttrName(node.name)}`);
      }
    }

    if (
      ts.isPropertyAssignment(node) &&
      USER_VISIBLE_OBJECT_KEYS.has(propName(node.name) ?? '') &&
      ts.isStringLiteralLike(node.initializer)
    ) {
      addViolation(violations, program, node.initializer, node.initializer.text, `object key ${propName(node.name)}`);
    }

    if (ts.isCallExpression(node) && isToastCall(node)) {
      const first = node.arguments[0];
      if (first && ts.isStringLiteralLike(first) && !insideTranslationCall(first)) {
        addViolation(violations, program, first, first.text, 'toast literal');
      }
    }

    ts.forEachChild(node, visit);
  }

  visit(program);
  return violations;
}

// Phase 6 M0.1: retired i18n group keys must not reappear. Checked BEFORE
// the route-file scan so this signal isn't masked by the in-flight copy
// migration backlog. If this fails, either the IA was reverted (fix the
// IA, not this list) or someone copy-pasted from an old branch.
const RETIRED_NAV_GROUPS = ['observe', 'data', 'automate'] as const;
const NAV_JSON_PATHS = ['src/i18n/zh-cn/nav.json', 'src/i18n/en-us/nav.json'] as const;
const navViolations: { file: string; key: string }[] = [];
for (const navPath of NAV_JSON_PATHS) {
  const navContent = ts.sys.readFile(join(WEB, navPath)) ?? '';
  let parsed: { groups?: Record<string, unknown> };
  try {
    parsed = JSON.parse(navContent) as { groups?: Record<string, unknown> };
  } catch {
    console.error(`check-migrated-copy: failed to parse ${navPath}`);
    process.exit(1);
  }
  const groups = parsed.groups ?? {};
  for (const retired of RETIRED_NAV_GROUPS) {
    if (retired in groups) navViolations.push({ file: navPath, key: `groups.${retired}` });
  }
}
if (navViolations.length > 0) {
  for (const v of navViolations) {
    console.error(`${v.file}  retired i18n key reappeared: "${v.key}"`);
  }
  console.error(`\ncheck-migrated-copy: ${navViolations.length} retired nav group key(s). Phase 3 IA collapsed 5 groups to 4 (home/investigate/pipeline/admin); these keys must stay removed.`);
  process.exit(1);
}

const files = [...new Set(TARGETS.flatMap(collectFiles))].sort();
const violations = files.flatMap(auditFile);

if (violations.length > 0) {
  for (const violation of violations) {
    console.error(`${violation.file}:${violation.line}  ${violation.context}: "${violation.text}"`);
  }
  console.error(`\ncheck-migrated-copy: ${violations.length} user-visible literal(s) in migrated routes. Move copy into i18n resources.`);
  process.exit(1);
}

console.log(`check-migrated-copy: ${files.length} migrated route files + ${NAV_JSON_PATHS.length} nav locales, 0 violations.`);
