import { Boxes, ExternalLink, GitBranch, Layers, type LucideIcon, MapPin, Network, Rocket, Server, Users } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ServiceDefinition, ServiceSummary, VersionSummary } from '@/api/apm';
import { cn } from '@/shell/lib/cn';
import { useUsers } from '@/shell/useUsers';
import { formatRelativeMicros } from '@/time/relative';

import { ENDPOINT_KIND_TONE } from './Overview';
import { deployedAt, entryKindsOf, firstOf, languageColor, languageOf, repositoryHref, repositoryOf, stackOf, versionsInView } from './serviceFacts';
import type { ServiceAddress } from './useService';
import { TIER_CLASS } from '../catalog/CatalogTable';
import { useOnCall, useTeams } from '../catalog/people';
import { healthReasonText } from '../health';
import { HEALTH_TEXT } from '../overview/model';
import { servicePath } from '../scope';

/**
 * What the service is, in the line after its name: its health, or that no
 * data came in the range (which says nothing of its health), how much it
 * matters, its system and environment, what it serves and is written in,
 * the version or versions in view, its team, how many instances run it, when
 * it was last deployed, where it runs and its repository. Each follows the
 * environment and version in view; what the telemetry reports wins over what
 * people set in the catalog.
 */
export function ServiceFacts({
  address,
  summary,
  versions,
  definition,
}: {
  address: ServiceAddress;
  summary: ServiceSummary | undefined;
  versions: readonly VersionSummary[];
  definition: ServiceDefinition | null;
}) {
  const { t, i18n } = useTranslation('apm');
  const { byId: teams } = useTeams();
  const { nameOf } = useUsers();
  const onCall = useOnCall(definition?.teamId ? [definition.teamId] : []);
  const chosen = address.scope.version || undefined;
  const reported = Boolean(summary && summary.red.requestCount > 0);
  const kinds = entryKindsOf(summary?.profile);
  const stack = stackOf(summary);
  const dot = languageColor(languageOf(summary));
  const inView = versionsInView(summary, chosen);
  const deployed = deployedAt(summary, versions, chosen);
  const region = firstOf(summary?.profile?.regions);
  const repository = repositoryOf(summary, definition?.repositoryUrl);
  const commit = summary?.profile?.commits[0]?.value;
  const team = definition?.teamId ? (teams.get(definition.teamId)?.name ?? definition.teamId) : null;
  const onCallUser = definition?.teamId ? onCall.get(definition.teamId) : undefined;
  const people = [
    definition?.ownerId ? `${t('definition.fields.owner')}: ${nameOf(definition.ownerId)}` : null,
    onCallUser ? `${t('definition.on_call')}: ${nameOf(onCallUser)}` : null,
  ].filter(Boolean);
  const instances = summary?.profile?.instanceCount ?? 0;

  return (
    <span data-apm-service-facts="" className="inline-flex min-w-0 items-center gap-x-3.5">
      {summary &&
        (reported ? (
          <span
            data-apm-service-health={summary.health}
            className={cn('font-strong', HEALTH_TEXT[summary.health])}
            title={summary.healthReasons?.map((reason) => healthReasonText(reason, t)).join(t('health.reason_separator')) || undefined}
          >
            {t(`health.${summary.health}`)}
          </span>
        ) : (
          <span
            data-apm-service-health="no_data"
            className="text-tx-3"
            title={t('service.facts.last_seen', { when: formatRelativeMicros(summary.lastSeenAt, i18n.language) })}
          >
            {t('service.facts.no_data')}
          </span>
        ))}
      {definition?.tier && (
        <span data-apm-service-tier={definition.tier} className={TIER_CLASS[definition.tier]} title={t('definition.fields.tier')}>
          {t(`definition.tier.${definition.tier}`)}
        </span>
      )}
      {definition?.archived && (
        <span data-apm-service-archived="" className="font-strong text-[var(--purple-ink)]" title={t('definition.archive_hint')}>
          {t('catalog.archived')}
        </span>
      )}
      <Fact icon={Layers} tone="text-[var(--chart-1)]" label={t('service.facts.system')} value={address.namespace} />
      <Fact icon={Server} tone="text-[var(--chart-2)]" label={t('service.facts.environment')} value={address.environment} />
      {kinds.length > 0 && (
        <Fact icon={Network} tone="text-[var(--chart-7)]" label={t('service.facts.type')} value={kinds.map((kind) => t(`service.facts.kinds.${kind}`)).join(' · ')}>
          {kinds.map((kind, index) => (
            <span key={kind} className={cn('font-strong', ENDPOINT_KIND_TONE[kind])}>
              {index > 0 && <span className="font-body text-tx-3"> · </span>}
              {t(`service.facts.kinds.${kind}`)}
            </span>
          ))}
        </Fact>
      )}
      {stack && (
        <span data-apm-service-fact="stack" className="inline-flex shrink-0 items-center gap-1.5 text-tx-1" title={`${t('service.facts.stack')}: ${stack}`}>
          <span aria-hidden className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ background: dot ?? 'var(--tx-3)' }} />
          <span className="sr-only">{t('service.facts.stack')}</span>
          {stack}
        </span>
      )}
      {inView.length > 0 && (
        <Fact
          icon={GitBranch}
          tone="text-[var(--purple-ink)]"
          label={t('service.facts.version')}
          value={inView.slice(0, 2).join(' · ') + (inView.length > 2 ? ` +${inView.length - 2}` : '')}
          detail={commit ? t('service.facts.commit', { commit: commit.slice(0, 12) }) : undefined}
          testId="version"
          strong
        />
      )}
      {team && <Fact icon={Users} tone="text-[var(--green-ink)]" label={t('service.facts.team')} value={team} detail={people.join(' · ') || undefined} />}
      {instances > 0 && (
        <Link
          to={servicePath(address.identity, 'runtime', address.searchParams)}
          data-apm-service-instances={instances}
          className="inline-flex shrink-0 items-center gap-1 text-tx-1 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none"
        >
          <Boxes aria-hidden className="h-3.5 w-3.5 shrink-0 text-[var(--blue-ink)]" />
          {t('service.facts.instances', { count: instances })}
        </Link>
      )}
      {deployed !== null && (
        <Fact
          icon={Rocket}
          tone="text-[var(--orange-ink)]"
          label={t('service.facts.deployed_label')}
          value={t('service.facts.deployed', { when: formatRelativeMicros(deployed, i18n.language) })}
        />
      )}
      {region && <Fact icon={MapPin} tone="text-[var(--chart-5)]" label={t('service.facts.region')} value={region.value + (region.more ? ` +${region.more}` : '')} />}
      {repository &&
        (repositoryHref(repository) ? (
          <a
            href={repositoryHref(repository)!}
            target="_blank"
            rel="noreferrer"
            title={repository}
            data-apm-service-repository=""
            className="inline-flex shrink-0 items-center gap-1 font-strong text-indigo-soft hover:underline focus-visible:underline focus-visible:outline-none"
          >
            {t('service.facts.repository')}
            <ExternalLink aria-hidden className="h-3 w-3 shrink-0" />
          </a>
        ) : (
          <span data-apm-service-repository="" title={repository} className="shrink-0 text-tx-1">
            {t('service.facts.repository')}
          </span>
        ))}
    </span>
  );
}

/** A fact with its icon in the colour of its kind and its value in the ink of the line. */
function Fact({
  icon: Icon,
  tone,
  label,
  value,
  detail,
  testId,
  strong = false,
  children,
}: {
  icon: LucideIcon;
  tone: string;
  label: string;
  value: string;
  detail?: string | undefined;
  testId?: string;
  strong?: boolean;
  children?: React.ReactNode;
}) {
  return (
    <span
      data-apm-service-fact={testId ?? label}
      className={cn('inline-flex min-w-0 shrink-0 items-center gap-1', strong ? 'font-strong text-tx-0' : 'text-tx-1')}
      title={detail ? `${label}: ${value} · ${detail}` : `${label}: ${value}`}
    >
      <Icon aria-hidden className={cn('h-3.5 w-3.5 shrink-0', tone)} />
      <span className="sr-only">{label}</span>
      <span className="max-w-[220px] truncate">{children ?? value}</span>
    </span>
  );
}
