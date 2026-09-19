/**
 * Blue is a semantic information/data color. Brand-primary and selected
 * controls use indigo. This gate catches the source patterns that previously
 * let a blue active tab or primary action drift into the product grammar.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const WEB = resolve(HERE, '..');
const SRC = join(WEB, 'src');

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (name === '__tests__' || name === '_demo') continue;
    const path = join(dir, name);
    const stat = statSync(path);
    if (stat.isDirectory()) walk(path, out);
    else if (name.endsWith('.tsx')) out.push(path);
  }
  return out;
}

const FORBIDDEN: Array<{ name: string; pattern: RegExp }> = [
  {
    name: 'blue filled primary/selection',
    pattern: /\bbg-blue(?:\/\d+)?\b[^\n]*(?:text-white|--blue-fg)|(?:text-white|--blue-fg)[^\n]*\bbg-blue(?:\/\d+)?\b/g,
  },
  {
    name: 'blue aria-current selection',
    pattern: /aria-current[\s\S]{0,280}\b(?:bg|text|border)-blue(?:-soft|\/\d+)?\b/g,
  },
  {
    name: 'blue primary variant',
    pattern: /variant\s*===?\s*['"]primary['"][\s\S]{0,320}\b(?:bg|text|border)-blue(?:-soft|\/\d+)?\b/g,
  },
];

let violations = 0;
const files = walk(SRC);
for (const file of files) {
  const source = readFileSync(file, 'utf8');
  for (const { name, pattern } of FORBIDDEN) {
    pattern.lastIndex = 0;
    for (const match of source.matchAll(pattern)) {
      const line = source.slice(0, match.index).split('\n').length;
      console.error(
        `${relative(WEB, file)}:${line}  ${name} — use indigo for brand/selected controls; reserve blue for information and data`,
      );
      violations += 1;
    }
  }
}

if (violations > 0) {
  console.error(`\n${violations} brand-primary color violation(s).`);
  process.exit(1);
}

console.log(`check-brand-primary: ${files.length} .tsx files, 0 violations.`);
