import { useMutation, useQuery } from '@tanstack/react-query';
import { Link2 } from 'lucide-react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import * as imApi from '@/api/agent/im';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { Button } from '@/shell/ui/button';

import { ModulePage } from '../operations/Pages';

/**
 * Where a one-time link sent in IM lands: the person signed in confirms that
 * the IM account is theirs, and Mole Agent answers it as them from then on.
 */
export function ImLinkPage() {
  const { t } = useTranslation('agent');
  const [params] = useSearchParams();
  const token = params.get('token')?.trim() ?? '';

  const offer = useQuery({
    queryKey: ['agent', 'im', 'link', token],
    queryFn: () => imApi.peekLink(token),
    enabled: token !== '',
    retry: false,
  });
  const confirm = useMutation({ mutationFn: () => imApi.confirmLink(token) });

  // Named as it reads in a sentence: in Chinese, Latin names take a space on each side.
  const platform = offer.data?.platform
    ? t(`im_link.platforms.${offer.data.platform}`)
    : t('im_link.platform_fallback');

  let body: ReactNode;

  if (confirm.isSuccess) {
    body = (
      <ProductState
        variant="empty"
        title={t('im_link.linked')}
        description={t('im_link.linked_hint', { platform })}
      />
    );
  } else if (token === '' || offer.isError) {
    body = (
      <ProductState
        variant="empty"
        title={t('im_link.expired')}
        description={t('im_link.expired_hint')}
      />
    );
  } else if (offer.isPending) {
    body = <ProductState variant="loading" />;
  } else {
    const account = offer.data.displayName
      ? `${offer.data.displayName} (${offer.data.externalIdentity})`
      : offer.data.externalIdentity;

    body = (
      <section
        data-im-link
        className="mx-auto flex max-w-[520px] flex-col items-center gap-3 py-[56px] text-center"
      >
        <Link2 className="h-6 w-6 text-tx-2" aria-hidden />
        <h2 className="m-0 text-sm font-display text-tx-0">
          {t('im_link.question', { platform, account })}
        </h2>
        <p className="m-0 text-xs leading-5 text-tx-2">
          {t('im_link.explain', {
            platform,
            connector: offer.data.connectorName,
            until: formatMicrosActive(offer.data.expiresAt, false),
          })}
        </p>
        {confirm.isError && (
          <p role="alert" className="m-0 text-xs text-[var(--red-ink)]">
            {toApiError(confirm.error).message}
          </p>
        )}
        <Button size="sm" disabled={confirm.isPending} onClick={() => confirm.mutate()}>
          {confirm.isPending ? t('im_link.linking') : t('im_link.confirm')}
        </Button>
      </section>
    );
  }

  return <ModulePage title={t('im_link.title')}>{body}</ModulePage>;
}
