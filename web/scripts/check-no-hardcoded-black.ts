/**
 * Gate against `text-black` / `bg-black` / `border-black` /
 * `color: black|#000` in any `web/src/**\/*.tsx`. Every color reference
 * goes through tokens, never
 * raw black.
 *
 * Run via `pnpm -C web exec tsx scripts/check-no-hardcoded-black.ts`
 * (also wired into `pnpm lint` as a pre-step in CI).
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const WEB = resolve(HERE, '..');
const SRC = join(WEB, 'src');

const PATTERNS: Array<{ name: string; re: RegExp }> = [
  // Tailwind class utilities — match `bg-black/40` etc. too. `\b` won't fire
  // across the `/` because `/` is a non-word char.
  { name: 'text-black', re: /\btext-black(\/\d{1,3})?\b/ },
  { name: 'bg-black', re: /\bbg-black(\/\d{1,3})?\b/ },
  { name: 'border-black', re: /\bborder-black(\/\d{1,3})?\b/ },
  // CSS inline / style props
  { name: "color: black", re: /\bcolor\s*:\s*black\b/i },
  { name: "color: #000", re: /\bcolor\s*:\s*#000\b/ },
  { name: "background: #000", re: /\bbackground(?:-color)?\s*:\s*#000\b/ },
];

function walkTsx(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (name === '__tests__' || name === '_demo') continue;
    const p = join(dir, name);
    const s = statSync(p);
    if (s.isDirectory()) walkTsx(p, out);
    else if (name.endsWith('.tsx')) out.push(p);
  }
  return out;
}

function main(): number {
  const files = walkTsx(SRC);
  let hits = 0;
  for (const f of files) {
    const src = readFileSync(f, 'utf8');
    const lines = src.split('\n');
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i]!;
      for (const { name, re } of PATTERNS) {
        if (re.test(line)) {
          console.error(`${relative(WEB, f)}:${i + 1}  hardcoded ${name} — use tokens (--tx-*, --bg-*, --overlay) instead`);
          hits++;
        }
      }
    }
  }
  if (hits > 0) {
    console.error(`\n${hits} hardcoded-black violation(s). Replace with token references.`);
    return 1;
  }
  console.log(`check-no-hardcoded-black: ${files.length} .tsx files, 0 violations.`);
  return 0;
}

process.exit(main());
