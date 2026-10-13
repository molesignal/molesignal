// @vitest-environment node
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import loadConfig from 'tailwindcss/loadConfig';
import { describe, expect, it } from 'vitest';

/**
 * Every file the Tailwind `content` list matches becomes a dependency of the
 * stylesheet in the dev server, and saving a dependency that no page has
 * loaded as a script reloads every open page. A unit test is never loaded by
 * a page, so `content` must not match one. It must keep matching every other
 * source file: one left out gets no classes generated, and a save of it never
 * reaches the stylesheet.
 *
 * The files are listed the way Vite lists them, not the way Tailwind scans
 * them. Tailwind reports each pattern as a directory plus a glob and reports
 * a `!` exclusion not at all; Vite then expands the glob on its own.
 */
type Glob = (
  pattern: string,
  options: { absolute: boolean; cwd: string; expandDirectories: boolean; ignore: string[] },
) => string[];

const WEB = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const SRC = path.join(WEB, 'src');

// The glob library as Vite resolves it; web/ does not depend on it directly.
const viteRequire = createRequire(createRequire(import.meta.url).resolve('vite/package.json'));
const { globSync } = viteRequire('tinyglobby') as { globSync: Glob };

const expand = (dir: string, glob: string) =>
  globSync(glob, { absolute: true, cwd: dir, expandDirectories: false, ignore: ['**/node_modules/**'] });

const GLOB_SYNTAX = /[*?[\]{}]|[!@+]\(/;
const isExclusion = (entry: string) => entry.startsWith('!') && entry[1] !== '(';

/** What the dev server registers as dependencies of the stylesheet for one `content` entry. */
function stylesheetDependencies(entry: string): string[] {
  const segments = entry.split('/');
  const firstGlob = segments.findIndex((segment) => GLOB_SYNTAX.test(segment));
  if (firstGlob === -1) return [path.resolve(WEB, entry)];

  return expand(path.resolve(WEB, ...segments.slice(0, firstGlob)), segments.slice(firstGlob).join('/'));
}

const isUnitTest = (file: string) =>
  /\.test\.tsx?$/.test(file) || path.basename(path.dirname(file)) === '__tests__';

const relative = (files: string[]) => files.map((file) => path.relative(WEB, file)).sort();

describe('tailwind content', () => {
  const { content } = loadConfig(path.join(WEB, 'tailwind.config.ts'));
  const entries = (Array.isArray(content) ? content : content.files).filter(
    (entry): entry is string => typeof entry === 'string' && !isExclusion(entry),
  );

  const dependencies = new Set(entries.flatMap(stylesheetDependencies));
  const sources = expand(SRC, '**/*.{ts,tsx}');

  it('finds the source tree', () => {
    expect(sources.filter(isUnitTest)).not.toHaveLength(0);
    expect(sources.filter((file) => !isUnitTest(file))).not.toHaveLength(0);
  });

  it('matches no unit test', () => {
    expect(relative(sources.filter((file) => isUnitTest(file) && dependencies.has(file)))).toEqual([]);
  });

  it('matches every other source file', () => {
    expect(relative(sources.filter((file) => !isUnitTest(file) && !dependencies.has(file)))).toEqual([]);
  });
});
