import type { FunctionLanguage, FunctionResp } from '@/api/functions';

/**
 * Which functions the list shows: all of them, the ones the workspace wrote, or the presets
 * the product ships with.
 */
export type FunctionView = 'all' | 'own' | 'builtin';

export const FUNCTION_VIEWS: readonly FunctionView[] = ['all', 'own', 'builtin'];

/** How many functions each view holds, before any search. */
export function viewCounts(functions: readonly FunctionResp[]): Record<FunctionView, number> {
  const builtin = functions.filter((item) => item.is_builtin).length;

  return { all: functions.length, own: functions.length - builtin, builtin };
}

/**
 * The functions of a view that a search matches. A function is found by its name and by what it
 * is said to do: a preset is looked for by its purpose more often than by its name.
 */
export function visibleFunctions(
  functions: readonly FunctionResp[],
  view: FunctionView,
  search: string,
): FunctionResp[] {
  const needle = search.trim().toLowerCase();

  return functions.filter((item) => {
    if (view === 'own' && item.is_builtin) return false;
    if (view === 'builtin' && !item.is_builtin) return false;
    if (!needle) return true;

    return (
      item.name.toLowerCase().includes(needle) || item.description.toLowerCase().includes(needle)
    );
  });
}

/** The name of a language as it is written, not as the API spells it. */
export function languageName(language: FunctionLanguage): string {
  return language === 'vrl' ? 'VRL' : 'JavaScript';
}

/** Where a new function that starts as a copy of another one is written. */
export function startFromPath(source: Pick<FunctionResp, 'id'>): string {
  return `/functions/new?from=${encodeURIComponent(source.id)}`;
}

/**
 * The name a copy starts with: `name-copy`, or the first of `name-copy-2`, `name-copy-3`… that
 * no function of the workspace has yet. Two functions of one name cannot be told apart where a
 * pipeline picks one.
 */
export function copyName(name: string, taken: readonly string[]): string {
  const used = new Set(taken);
  const base = `${name}-copy`;
  if (!used.has(base)) return base;

  for (let index = 2; ; index += 1) {
    const candidate = `${base}-${index}`;
    if (!used.has(candidate)) return candidate;
  }
}
