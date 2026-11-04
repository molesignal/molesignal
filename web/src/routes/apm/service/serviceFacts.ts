import type { ProfileValue, ServiceProfile, ServiceSummary, TransactionIdentity, VersionSummary } from '@/api/apm';

/** `telemetry.sdk.language` as people write the language. */
const LANGUAGE_NAMES: Record<string, string> = {
  rust: 'Rust',
  java: 'Java',
  go: 'Go',
  python: 'Python',
  nodejs: 'Node.js',
  dotnet: '.NET',
  ruby: 'Ruby',
  php: 'PHP',
  cpp: 'C++',
  erlang: 'Erlang',
  swift: 'Swift',
  webjs: 'JavaScript',
};

/** The colour each language is known by (GitHub's), drawn as a dot before its name. */
const LANGUAGE_COLORS: Record<string, string> = {
  rust: '#dea584',
  java: '#b07219',
  go: '#00add8',
  python: '#3572a5',
  nodejs: '#3c873a',
  dotnet: '#512bd4',
  ruby: '#cc342d',
  php: '#4f5d95',
  cpp: '#f34b7d',
  erlang: '#b83998',
  swift: '#f05138',
  webjs: '#f1e05a',
};

/** The order a service's kinds of entry are said in. */
const KIND_ORDER: readonly TransactionIdentity['kind'][] = ['http', 'rpc', 'messaging', 'span'];

export function languageName(code: string): string {
  return LANGUAGE_NAMES[code.toLowerCase()] ?? code;
}

export function languageColor(code: string | null | undefined): string | null {
  return code ? (LANGUAGE_COLORS[code.toLowerCase()] ?? null) : null;
}

/** The language code a service reports, its most common one when its instances differ. */
export function languageOf(summary: ServiceSummary | null | undefined): string | null {
  return summary?.profile?.languages[0]?.value ?? summary?.instrumentation.runtimeLanguage ?? null;
}

/** The first value of a ranked list, and how many more there are. */
export function firstOf(values: readonly ProfileValue[] | undefined): { value: string; more: number } | null {
  const first = values?.[0];
  return first ? { value: first.value, more: values.length - 1 } : null;
}

/** What a service is written in and served by: `Rust / Axum`. */
export function stackOf(summary: ServiceSummary | null | undefined): string | null {
  const language = languageOf(summary);
  const framework = summary?.profile?.frameworks[0]?.value;
  const parts = [language ? languageName(language) : null, framework ?? null].filter((part): part is string => Boolean(part));

  return parts.length ? parts.join(' / ') : null;
}

/** The kinds of entry a service serves, in their usual order. */
export function entryKindsOf(profile: ServiceProfile | null | undefined): TransactionIdentity['kind'][] {
  return KIND_ORDER.filter((kind) => profile?.entryKinds.includes(kind));
}

/**
 * The versions in view: the one chosen, or those the instances ran within the
 * range, the most widely run first; a rollout shows two.
 */
export function versionsInView(summary: ServiceSummary | undefined, chosen: string | undefined): string[] {
  if (chosen) return [chosen];
  const ran = summary?.profile?.versions.map((version) => version.value) ?? [];
  if (ran.length) return ran;
  return summary?.currentVersion ? [summary.currentVersion] : (summary?.versions ?? []);
}

/** When the version in view was first seen, or the newest one when none is chosen: the last deploy. */
export function deployedAt(summary: ServiceSummary | undefined, versions: readonly VersionSummary[], chosen: string | undefined): number | null {
  if (chosen) return versions.find((version) => version.version === chosen)?.firstSeenAt ?? null;
  return summary?.currentVersionSince ?? null;
}

/** The repository the service's telemetry names, else the one people set in its catalog entry. */
export function repositoryOf(summary: ServiceSummary | undefined, configured: string | null | undefined): string | null {
  return summary?.profile?.repositories[0]?.value ?? configured ?? null;
}

/**
 * A repository address as a page to open: an http(s) address without its
 * credentials, and an SSH or git address (`git@host:org/repo.git`) as the
 * https page of the same path. `null` for anything else.
 */
export function repositoryHref(address: string): string | null {
  const value = address.trim();
  const url = /^([a-z][a-z0-9+.-]*):\/\/([^/?#]*)(.*)$/i.exec(value);
  let host: string;
  let path: string;

  if (url) {
    const [, scheme = '', authority = '', tail = ''] = url;
    const hostPort = authority.slice(authority.lastIndexOf('@') + 1);
    if (/^https?$/i.test(scheme)) return hostPort ? `${scheme.toLowerCase()}://${hostPort}${tail}` : null;
    if (!/^(?:ssh|git|git\+ssh|ssh\+git)$/i.test(scheme)) return null;
    // The SSH port is not the web's.
    host = hostPort.replace(/:\d+$/, '');
    path = tail;
  } else {
    const scp = /^[^@\s]+@([^:\s]+):(.+)$/.exec(value);
    if (!scp) return null;
    host = scp[1] ?? '';
    path = `/${(scp[2] ?? '').replace(/^\/+/, '')}`;
  }

  return host ? `https://${host}${path.replace(/\.git$/, '')}` : null;
}
