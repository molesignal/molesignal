import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { useParams, useSearchParams } from 'react-router-dom';

import type { MetadataStripItem } from '@/admin';
import { ENTITY_KINDS, type EntityKind, entitiesApi, entityQueryKeys } from '@/api/entities';
import { toApiError } from '@/lib/http';
import { useTimeFormatter } from '@/lib/time';
import type { ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { ChromeButton, Dot } from '@/shell/chrome';
import { useAuthStore } from '@/stores/auth';
import { formatRelativeMicros } from '@/time/relative';

import { EntityBody } from './EntityBody';
import { isCurrent } from './model';

/** One entity's page: `/infrastructure/:kind?key=`. */
export function EntityPage() {
  const { t, i18n } = useTranslation('infrastructure');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const { kind: kindParam = '' } = useParams();
  const [params] = useSearchParams();
  const key = params.get('key') ?? '';
  const kind = ENTITY_KINDS.includes(kindParam as EntityKind) ? (kindParam as EntityKind) : null;
  const time = useTimeFormatter();
  const now = Date.now() * 1_000;

  const query = useQuery({
    queryKey: entityQueryKeys.detail(orgId, kindParam, key),
    queryFn: () => entitiesApi.detail(kindParam, key),
    enabled: Boolean(orgId && kind && key),
    retry: (count, error) => toApiError(error).status !== 404 && count < 2,
    staleTime: 30_000,
  });
  const detail = query.data;
  const missing = !kind || !key || (query.isError && toApiError(query.error).status === 404);

  const state: ProductStateProps | null = missing
    ? { variant: 'empty', title: t('detail.not_found_title'), description: t('detail.not_found_description') }
    : query.isLoading
      ? { variant: 'loading' }
      : query.isError || !detail
        ? {
            variant: 'error',
            title: t('detail.error_title'),
            error: query.error,
            action: <ChromeButton onClick={() => void query.refetch()}>{t('list.retry')}</ChromeButton>,
          }
        : null;

  const entity = detail?.entity;
  const metadata: MetadataStripItem[] | undefined = entity
    ? [
        { label: t('detail.kind'), value: t(`kind.${entity.kind}`) },
        { label: t('detail.key'), value: <span className="font-mono">{entity.key}</span> },
        {
          label: t('detail.last_seen'),
          value: (
            <span className="inline-flex items-center gap-1.5">
              <Dot tone={isCurrent(entity, now) ? 'green' : 'dim'} />
              {isCurrent(entity, now) ? t('status.current') : t('status.stale', { when: formatRelativeMicros(entity.lastSeenAt, i18n.language) })}
            </span>
          ),
        },
        { label: t('detail.first_seen'), value: time.micros(entity.firstSeenAt) },
      ]
    : undefined;

  return (
    <DetailPage appearance="surface" title={entity?.name ?? (key.split('/').pop() || t('title'))} metadata={metadata} state={state}>
      {detail && <EntityBody detail={detail} now={now} />}
    </DetailPage>
  );
}
