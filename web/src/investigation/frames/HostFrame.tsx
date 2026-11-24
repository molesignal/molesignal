import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { ENTITY_KINDS, type EntityKind, entitiesApi, entityQueryKeys } from '@/api/entities';
import type { FrameProps } from '@/investigation/frame';
import { EntityBody } from '@/routes/infrastructure/EntityBody';
import { entityHref } from '@/routes/infrastructure/model';
import { EmptyState } from '@/shell/EmptyState';
import { ErrorState } from '@/shell/ErrorState';
import { LoadingState } from '@/shell/LoadingState';
import { useAuthStore } from '@/stores/auth';

/**
 * Where something runs, on the investigation stack: a host, or a cluster,
 * node, pod or container (`params.kind`, a host when left out), by the key
 * the infrastructure pages use (`params.key`, or `params.host` for a host),
 * with the same body as its page.
 */
export function HostFrame({ frame }: FrameProps) {
  const { t } = useTranslation('infrastructure');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const asked = typeof frame.params.kind === 'string' ? frame.params.kind : 'host';
  const kind: EntityKind = ENTITY_KINDS.includes(asked as EntityKind) ? (asked as EntityKind) : 'host';
  const key = [frame.params.key, frame.params.host].find((value): value is string => typeof value === 'string' && value !== '') ?? '';

  const query = useQuery({
    queryKey: entityQueryKeys.detail(orgId, kind, key),
    queryFn: () => entitiesApi.detail(kind, key),
    enabled: Boolean(orgId && key),
    retry: false,
    staleTime: 30_000,
  });

  if (!key) return <EmptyState title={t('detail.not_found_title')} description={t('detail.not_found_description')} />;
  if (query.isLoading) return <LoadingState variant="list" rows={4} />;
  if (query.isError || !query.data) {
    return <ErrorState error={query.error} title={t('detail.error_title')} onRetry={() => void query.refetch()} />;
  }

  return (
    <div data-host-frame={key} className="flex h-full flex-col gap-3 overflow-y-auto p-4">
      <div className="flex items-center gap-2">
        <h2 className="min-w-0 truncate font-sans text-sm font-strong text-tx-0" title={key}>
          {query.data.entity.name}
        </h2>
        <span className="font-sans text-xs text-tx-3">{t(`kind.${kind}`)}</span>
        <Link to={entityHref(kind, key)} className="ml-auto font-sans text-xs font-strong text-tx-2 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none">
          {t('detail.open_page')}
        </Link>
      </div>
      <EntityBody detail={query.data} now={Date.now() * 1_000} />
    </div>
  );
}
