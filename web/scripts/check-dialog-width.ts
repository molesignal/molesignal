/**
 * A dialog given a width wider than the shared cap must lift the cap too.
 * `DialogContent` (shell/ui/dialog.tsx) carries `max-w-lg`, 448px at the 14px
 * root. A caller's `w-[…]` replaces the width but not that cap (tailwind-merge
 * keeps the two apart), so a dialog meant to be 600px wide is drawn at 448 and
 * a long token or command runs out of its box. Next to such a width the caller
 * writes `max-w-none`, or a `max-w-[…]` of its own.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import ts from 'typescript';

const HERE = dirname(fileURLToPath(import.meta.url));
const WEB = resolve(HERE, '..');
const SRC = join(WEB, 'src');
const ROOT_PX = 14;
const CAP_PX = 32 * ROOT_PX;

function walkTsx(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (name === '__tests__' || name === '_demo') continue;
    const path = join(dir, name);
    const stat = statSync(path);
    if (stat.isDirectory()) walkTsx(path, out);
    else if (name.endsWith('.tsx')) out.push(path);
  }
  return out;
}

/** Every class a `className` can hold: the text of each string in it, `cn(…)` arguments and branches included. */
function classesOf(value: ts.Node | undefined): string[] {
  const parts: string[] = [];
  const visit = (node: ts.Node) => {
    if (
      ts.isStringLiteral(node) ||
      ts.isNoSubstitutionTemplateLiteral(node) ||
      ts.isTemplateHead(node) ||
      ts.isTemplateMiddle(node) ||
      ts.isTemplateTail(node)
    ) {
      parts.push(node.text);
    }
    ts.forEachChild(node, visit);
  };
  if (value) visit(value);
  return parts.join(' ').split(/\s+/).filter(Boolean);
}

/** The utility without its variants (`sm:`, `data-[state=open]:`) or `!`. */
function utilityOf(cls: string): string {
  return cls.replace(/^(?:(?:[a-z0-9-]*\[[^\]]*\]|[a-z0-9-]+):)+/, '').replace(/^!/, '');
}

/** A length in px; a viewport share, a percentage or a calc() can be as wide as the window. */
function lengthPx(value: string): number {
  const match = /^(\d+(?:\.\d+)?)(px|rem)$/.exec(value.trim());
  if (!match) return Number.POSITIVE_INFINITY;
  return Number(match[1]) * (match[2] === 'rem' ? ROOT_PX : 1);
}

function topLevelArguments(list: string): string[] {
  const args: string[] = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < list.length; index++) {
    const char = list[index];
    if (char === '(') depth++;
    else if (char === ')') depth--;
    else if (char === ',' && depth === 0) {
      args.push(list.slice(start, index));
      start = index + 1;
    }
  }
  args.push(list.slice(start));
  return args;
}

/**
 * How wide a width utility can make the dialog, or null for one that sets no
 * width of its own. `w-full` is the cap's own width, and the spacing scale
 * ends at 336px, under the cap.
 */
function widthPx(utility: string): number | null {
  if (utility === 'w-screen' || /^w-\d+\/\d+$/.test(utility)) return Number.POSITIVE_INFINITY;
  const arbitrary = /^w-\[(.+)\]$/.exec(utility);
  if (!arbitrary) return null;
  const value = arbitrary[1]!.replace(/_/g, ' ');
  const min = /^min\((.*)\)$/.exec(value);
  if (!min) return lengthPx(value);
  return Math.min(...topLevelArguments(min[1]!).map(lengthPx));
}

function main(): number {
  const files = walkTsx(SRC);
  let dialogs = 0;
  let violations = 0;

  for (const file of files) {
    const text = readFileSync(file, 'utf8');
    if (!text.includes('<DialogContent')) continue;
    const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);

    const visit = (node: ts.Node) => {
      if (
        (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) &&
        node.tagName.getText(source) === 'DialogContent'
      ) {
        dialogs++;
        const className = node.attributes.properties.find(
          (property): property is ts.JsxAttribute =>
            ts.isJsxAttribute(property) && property.name.getText(source) === 'className',
        );
        const classes = classesOf(className?.initializer);
        const wide = classes.find((cls) => (widthPx(utilityOf(cls)) ?? 0) > CAP_PX);
        if (wide && !classes.some((cls) => utilityOf(cls).startsWith('max-w-'))) {
          const line = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
          console.error(
            `${relative(WEB, file)}:${line}  DialogContent ${wide} is held to ${CAP_PX}px by max-w-lg — add max-w-none, or a max-w-[…] of its own`,
          );
          violations++;
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }

  if (violations > 0) {
    console.error(`\n${violations} dialog width violation(s).`);
    return 1;
  }

  console.log(`check-dialog-width: ${files.length} .tsx files, ${dialogs} dialogs, 0 widths held by the cap.`);
  return 0;
}

process.exit(main());
