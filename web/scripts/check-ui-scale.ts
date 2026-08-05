/**
 * Prevent the compact pre-refresh type scale from creeping back into UI
 * source. Intentional axes, keyboard hints and diagnostics use the semantic
 * `type-micro` role; normal supporting copy starts at `text-xs`.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const WEB = resolve(HERE, '..');
const SRC = join(WEB, 'src');
const RAW_MICRO = /\btext-\[(?:[0-9](?:\.\d+)?|10(?:\.0+)?|11(?:\.0+)?)px\](?![\w-])/g;

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

function main(): number {
  const files = walkTsx(SRC);
  let hits = 0;

  for (const file of files) {
    const lines = readFileSync(file, 'utf8').split('\n');
    lines.forEach((line, index) => {
      RAW_MICRO.lastIndex = 0;
      for (const match of line.matchAll(RAW_MICRO)) {
        console.error(
          `${relative(WEB, file)}:${index + 1}  raw micro type ${match[0]} — use text-xs, or the intentional type-micro role for axes/kbd/diagnostics`,
        );
        hits++;
      }
    });
  }

  if (hits > 0) {
    console.error(`\n${hits} raw micro-type violation(s).`);
    return 1;
  }

  console.log(`check-ui-scale: ${files.length} .tsx files, 0 raw 9–11px utilities.`);
  return 0;
}

process.exit(main());
