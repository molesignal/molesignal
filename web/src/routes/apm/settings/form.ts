import type { ApmLimits, ApmSettings, EnvironmentMapping, NamingRule, ServiceAlias } from '@/api/apmSettings';
import { APM_LIMIT_MAX } from '@/api/apmSettings';

const MINUTE_MICROS = 60_000_000;

/** A number typed in a field; null when it is none. */
export function parseNumber(text: string): number | null {
  const trimmed = text.trim();
  const value = Number(trimmed);

  return trimmed !== '' && Number.isFinite(value) ? value : null;
}

export type ThresholdIssue = 'error_rates' | 'p95' | 'apdex' | 'long_task';

/** What the server would refuse in the thresholds; none when it takes them. */
export function thresholdIssues(settings: Pick<ApmSettings, 'health' | 'apdexThreshold' | 'longTaskTime'>): ThresholdIssue[] {
  const { health } = settings;
  const issues: ThresholdIssue[] = [];

  if (!(health.errorRateWarning > 0 && health.errorRateWarning < health.errorRateCritical && health.errorRateCritical <= 1)) {
    issues.push('error_rates');
  }

  if (!(health.p95WarningTime > 0 && health.p95WarningTime < health.p95CriticalTime && health.p95CriticalTime <= 10 * MINUTE_MICROS)) {
    issues.push('p95');
  }

  if (!(settings.apdexThreshold > 0 && settings.apdexThreshold <= MINUTE_MICROS)) issues.push('apdex');
  if (!(settings.longTaskTime >= 1_000 && settings.longTaskTime <= 10 * MINUTE_MICROS)) issues.push('long_task');

  return issues;
}

/**
 * A naming pattern as JavaScript reads it. The server reads Rust regular
 * expressions, which also name a group `(?P<name>…)`; what JavaScript cannot
 * read here may still be fine there, so this only previews.
 */
function jsPattern(pattern: string): RegExp | null {
  try {
    return new RegExp(pattern.replace(/\(\?P</g, '(?<'));
  } catch {
    return null;
  }
}

/** A replacement as JavaScript writes it: `${name}` and `$name` name a group. */
function jsReplacement(replacement: string): string {
  return replacement.replace(/\$\{(\w+)\}/g, (_, name: string) => (/^\d+$/.test(name) ? `$${name}` : `$<${name}>`)).replace(/\$([A-Za-z_]\w*)/g, '$<$1>');
}

export function patternReads(pattern: string): boolean {
  return pattern.trim() !== '' && jsPattern(pattern) !== null;
}

/**
 * The name a service is read by under `rules`, as the server renames it: the
 * first rule whose pattern matches rewrites it; a rewrite to nothing or to
 * the same name keeps it, and so does a pattern this cannot read.
 */
export function renamed(rules: readonly NamingRule[], name: string): string | null {
  for (const rule of rules) {
    const pattern = jsPattern(rule.pattern);

    if (!pattern || !pattern.test(name)) continue;

    const result = name.replace(pattern, jsReplacement(rule.replacement)).trim();

    return result !== '' && result !== name ? result : null;
  }

  return null;
}

/**
 * The name before a suffix a deployment generates, as Kubernetes names its
 * pods (`checkout-7d9f8b6c5-x2x7k`) or a release its hash (`checkout-5f4d2`):
 * up to two trailing parts of five or more letters and digits, with a digit.
 */
export function generatedBase(name: string): string | null {
  const parts = name.split('-');
  let cut = 0;

  while (cut < 2 && parts.length - cut > 1) {
    const part = parts[parts.length - 1 - cut] ?? '';

    if (!/^[a-z0-9]{5,}$/.test(part) || !/\d/.test(part)) break;
    cut += 1;
  }

  return cut > 0 ? parts.slice(0, parts.length - cut).join('-') : null;
}

/** A rule that reads `name`'s generated suffix away, for a name that has one. */
export function suffixRule(name: string): NamingRule | null {
  const base = generatedBase(name);

  if (!base) return null;

  return { pattern: `^${base.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}(-[a-z0-9]{5,}){1,2}$`, replacement: base };
}

/** Environments written more than one way, case aside: each group of names. */
export function caseVariants(environments: readonly string[]): string[][] {
  const groups = new Map<string, Set<string>>();

  for (const environment of environments) {
    const key = environment.trim().toLocaleLowerCase();
    if (!key) continue;
    groups.set(key, (groups.get(key) ?? new Set()).add(environment));
  }

  return [...groups.values()].filter((names) => names.size > 1).map((names) => [...names].sort());
}

/** The environments mapped more than once, case aside. */
export function mappedTwice(map: readonly EnvironmentMapping[]): Set<number> {
  const seen = new Map<string, number>();
  const twice = new Set<number>();

  map.forEach((mapping, index) => {
    const key = mapping.from.trim().toLocaleLowerCase();
    const first = seen.get(key);

    if (key && first !== undefined) {
      twice.add(first);
      twice.add(index);
    }

    if (first === undefined) seen.set(key, index);
  });

  return twice;
}

export type AliasIssue = 'incomplete' | 'itself' | 'twice' | 'chained';

/** What is wrong with each alias, as the server would refuse it, by its place. */
export function aliasIssues(aliases: readonly ServiceAlias[]): Map<number, AliasIssue> {
  const namespace = (value: string) => value.trim() || 'default';
  const key = (ns: string, name: string) => `${namespace(ns)}\u0000${name.trim()}`;
  const names = aliases.map((alias) => key(alias.aliasNamespace, alias.alias));
  const issues = new Map<number, AliasIssue>();

  aliases.forEach((alias, index) => {
    const own = names[index] ?? '';
    const target = key(alias.namespace, alias.service);

    if (!alias.alias.trim() || !alias.service.trim()) issues.set(index, 'incomplete');
    else if (own === target) issues.set(index, 'itself');
    else if (names.indexOf(own) < index) issues.set(index, 'twice');
    else if (names.includes(target)) issues.set(index, 'chained');
  });

  return issues;
}

/** The limits outside 1 to the most a limit may be, or not whole. */
export function limitIssues(limits: ApmLimits): Array<keyof ApmLimits> {
  return (Object.keys(limits) as Array<keyof ApmLimits>).filter((key) => {
    const value = limits[key];
    return !Number.isInteger(value) || value < 1 || value > APM_LIMIT_MAX;
  });
}
