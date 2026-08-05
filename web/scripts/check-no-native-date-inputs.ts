/**
 * Date and time controls must use the app-localized UI component. Native
 * browser pickers inherit browser/OS locale and render inconsistently across
 * platforms, so they are not allowed in application JSX.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const WEB = resolve(HERE, '..');
const SRC = join(WEB, 'src');
const NATIVE_DATE_TYPE =
  /\btype\s*=\s*(?:"(?:date|datetime-local|time|month|week)"|'(?:date|datetime-local|time|month|week)'|\{\s*["'](?:date|datetime-local|time|month|week)["']\s*\})/g;

function walkTsx(directory: string, output: string[] = []): string[] {
  for (const name of readdirSync(directory)) {
    if (name === '__tests__' || name === '_demo') continue;
    const path = join(directory, name);
    const stat = statSync(path);
    if (stat.isDirectory()) walkTsx(path, output);
    else if (name.endsWith('.tsx')) output.push(path);
  }
  return output;
}

function main(): number {
  const files = walkTsx(SRC);
  let violations = 0;

  for (const file of files) {
    const source = readFileSync(file, 'utf8');
    NATIVE_DATE_TYPE.lastIndex = 0;
    for (const match of source.matchAll(NATIVE_DATE_TYPE)) {
      const line = source.slice(0, match.index).split('\n').length;
      console.error(
        `${relative(WEB, file)}:${line}  native ${match[0]} is not allowed — use DateTimePicker so UI and locale follow the app`,
      );
      violations++;
    }
  }

  if (violations > 0) {
    console.error(`\n${violations} native date/time input violation(s).`);
    return 1;
  }

  console.log(
    `check-no-native-date-inputs: ${files.length} .tsx files, 0 violations.`,
  );
  return 0;
}

process.exit(main());
