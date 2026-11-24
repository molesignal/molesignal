import { useQuery } from '@tanstack/react-query';
import { ArrowUpRight, Boxes, GitCommitVertical, Layers, ListTree, Tags } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { changesApi, changesQueryKeys } from '@/api/changes';
import type { EntityDetail, EntityNeighbour } from '@/api/entities';
import { ApmCard } from '@/routes/apm/frame/Page';
import { CopyableValue } from '@/routes/settings/_atoms';
import { CHANGE_ICON, changeHref, changeSummary, changeTone } from '@/shell/changes/model';
import { cn } from '@/shell/lib/cn';
import { useAuthStore } from '@/stores/auth';
import { formatRelativeMicros } from '@/time/relative';

import { entityHref, logsHref, metricsHref, serviceHref, serviceLabel } from './model';

const DAY_MICROS = 86_400_000_000;
const MOST_CHANGES = 10;
const ROW = 'flex h-8 items-center gap-2 border-b border-[var(--row-rule)] px-3 font-sans text-xs last:border-b-0';
const LINK = 'font-strong text-tx-2 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none';

/**
 * One entity's body, on its page and on the investigation stack: what it runs
 * on and what runs on it, the services on it with their latest changes, its
 * resource attributes, and its logs and metrics.
 */
export function EntityBody({ detail, now }: { detail: EntityDetail; now: number }) {
  const { t } = useTranslation('infrastructure');
  const { entity } = detail;
  const logs = logsHref(entity, now);

  return (
    <div className="flex flex-col gap-2" data-entity={`${entity.kind}:${entity.key}`}>
      <div className="flex items-center gap-3 px-1 font-sans text-xs">
        {logs && (
          <Link to={logs} data-entity-logs="" className={LINK}>
            {t('detail.logs')}
          </Link>
        )}
        <Link to={metricsHref(entity, now)} data-entity-metrics="" className={LINK}>
          {t('detail.metrics')}
        </Link>
      </div>
      <div className="grid gap-2 lg:grid-cols-2">
        <Neighbours icon={Layers} title={t('detail.parents')} empty={t('detail.parents_empty')} items={detail.parents} testId="parents" />
        <Neighbours icon={ListTree} title={t('detail.children')} empty={t('detail.children_empty')} items={detail.children} testId="children" />
      </div>
      <ApmCard icon={Boxes} title={t('detail.services')} description={t('detail.services_description')} testId="entity-services">
        {detail.services.length === 0 ? (
          <p className="px-3 py-3 font-sans text-xs text-tx-3">{t('detail.services_empty')}</p>
        ) : (
          <ul>
            {detail.services.map((service) => (
              <li key={`${service.namespace}/${service.name}/${service.environment}`} className={ROW}>
                <Link to={serviceHref(service)} data-entity-service={service.name} className="min-w-0 truncate font-strong text-tx-0 hover:underline focus-visible:underline focus-visible:outline-none">
                  {serviceLabel(service)}
                </Link>
              </li>
            ))}
          </ul>
        )}
      </ApmCard>
      <ServiceChanges detail={detail} now={now} />
      <ApmCard icon={Tags} title={t('detail.attributes')} testId="entity-attributes">
        <dl>
          {Object.entries(entity.attributes).map(([name, value]) => (
            <div key={name} className={cn(ROW, 'gap-3')}>
              <dt className="w-[180px] shrink-0 truncate font-mono text-tx-3">{name}</dt>
              <dd className="min-w-0 flex-1">
                <CopyableValue value={value} copyLabel={t('detail.copy')} copiedLabel={t('detail.copied')} />
              </dd>
            </div>
          ))}
        </dl>
      </ApmCard>
    </div>
  );
}

function Neighbours({
  icon,
  title,
  empty,
  items,
  testId,
}: {
  icon: typeof Layers;
  title: string;
  empty: string;
  items: readonly EntityNeighbour[];
  testId: string;
}) {
  const { t } = useTranslation('infrastructure');

  return (
    <ApmCard icon={icon} title={title} testId={`entity-${testId}`}>
      {items.length === 0 ? (
        <p className="px-3 py-3 font-sans text-xs text-tx-3">{empty}</p>
      ) : (
        <ul className="max-h-[320px] overflow-y-auto">
          {items.map((item) => (
            <li key={`${item.kind}:${item.key}`} className={ROW}>
              <span className="w-[72px] shrink-0 text-tx-3">{t(`kind.${item.kind}`)}</span>
              <Link to={entityHref(item.kind, item.key)} data-entity-link={item.key} title={item.key} className="min-w-0 flex-1 truncate font-strong text-tx-0 hover:underline focus-visible:underline focus-visible:outline-none">
                {item.name}
              </Link>
              <ArrowUpRight aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
            </li>
          ))}
        </ul>
      )}
    </ApmCard>
  );
}

/** The latest changes of the services on the entity, over the last day. */
function ServiceChanges({ detail, now }: { detail: EntityDetail; now: number }) {
  const { t, i18n } = useTranslation('infrastructure');
  const { t: tc } = useTranslation('common');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const names = [...new Set(detail.services.map((service) => service.name))].slice(0, 50);
  const params = { from: now - DAY_MICROS, to: now, services: names, limit: MOST_CHANGES };

  const query = useQuery({
    queryKey: changesQueryKeys.list(orgId, params),
    queryFn: () => changesApi.list(params),
    enabled: Boolean(orgId) && names.length > 0,
    retry: false,
    staleTime: 60_000,
  });

  if (names.length === 0) return null;
  const changes = Array.isArray(query.data) ? query.data.slice(0, MOST_CHANGES) : [];

  return (
    <ApmCard icon={GitCommitVertical} title={t('detail.changes')} description={t('detail.changes_description')} testId="entity-changes">
      {changes.length === 0 ? (
        <p className="px-3 py-3 font-sans text-xs text-tx-3">{t('detail.changes_empty')}</p>
      ) : (
        <ul>
          {changes.map((change) => {
            const Icon = CHANGE_ICON[change.kind];
            const href = changeHref(change);
            const summary = changeSummary(change, tc);

            return (
              <li key={change.id} data-entity-change={change.kind} className={ROW}>
                <Icon aria-hidden className={cn('h-3.5 w-3.5 shrink-0', changeTone(change))} />
                {href ? (
                  <Link to={href} className="min-w-0 flex-1 truncate text-tx-0 hover:underline focus-visible:underline focus-visible:outline-none">
                    {summary}
                  </Link>
                ) : (
                  <span className="min-w-0 flex-1 truncate text-tx-0">{summary}</span>
                )}
                <span className="shrink-0 tabular-nums text-tx-3">{formatRelativeMicros(change.at, i18n.language)}</span>
              </li>
            );
          })}
        </ul>
      )}
    </ApmCard>
  );
}
