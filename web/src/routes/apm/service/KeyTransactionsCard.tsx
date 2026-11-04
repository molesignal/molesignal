import { Star } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { TransactionSummary } from '@/api/apm';
import { cn } from '@/shell/lib/cn';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { formatCount, formatDuration, formatRate } from '../format';
import { transactionKey, useKeyTransactions } from './keyTransactions';
import { ENDPOINT_KIND_TONE } from './Overview';
import type { ServiceAddress } from './useService';
import { OpenRow, StarToggle } from '../frame/OpenRow';
import { ApmCard } from '../frame/Page';
import { useAddressedDrawer } from '../frame/useApmScope';

/**
 * The endpoints starred as the service's key transactions, each with what it
 * served in the range: a line opens it, the star takes it off. Endpoints are
 * starred in their list or their drawer.
 */
export function KeyTransactionsCard({ address, transactions }: { address: ServiceAddress; transactions: readonly TransactionSummary[] }) {
  const { t } = useTranslation('apm');
  const keyed = useKeyTransactions(address);
  const drawer = useAddressedDrawer('endpoint');
  const starred = keyed.definition?.keyTransactions ?? [];
  const served = new Map(transactions.map((item) => [transactionKey(item.transaction), item]));

  return (
    <ApmCard
      icon={Star}
      tone="text-[var(--yellow-ink)]"
      title={t('settings.key_transactions.title')}
      description={t('settings.key_transactions.description')}
      testId="service-key-transactions"
    >
      {starred.length === 0 ? (
        <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('settings.key_transactions.empty')}</p>
      ) : (
        <div role="table" aria-label={t('settings.key_transactions.title')}>
          {starred.map((transaction) => {
            const item = served.get(transactionKey(transaction));

            return (
              <OpenRow
                key={transactionKey(transaction)}
                data-apm-key-transaction={transaction.name}
                onOpen={() => drawer.open(transaction.name, { kind: transaction.kind })}
                className="grid h-8 cursor-pointer grid-cols-[24px_56px_minmax(0,1fr)_72px_64px_72px] items-center gap-3 border-b border-bd-0 px-3 font-sans text-xs outline-none last:border-b-0 hover:bg-bg-2 focus-visible:bg-bg-2"
              >
                <span role="cell" className="-ml-1 flex items-center">
                  <StarToggle on label={keyed.title(transaction)} onToggle={() => keyed.toggle(transaction)} disabled={!keyed.canChange} />
                </span>
                <span role="cell" className={cn('truncate font-strong', ENDPOINT_KIND_TONE[transaction.kind])}>
                  {t(`endpoints.kind.${transaction.kind}`)}
                </span>
                <span role="cell" className="min-w-0">
                  <TraceOperationName mono={false} operation={transaction.name} className="min-w-0 text-tx-0" />
                </span>
                {item ? (
                  <>
                    <span role="cell" className="text-right tabular-nums text-tx-1" title={t('endpoints.columns.requests')}>
                      {formatCount(item.red.requestCount)}
                    </span>
                    <span
                      role="cell"
                      className={cn('text-right tabular-nums', item.red.errorCount > 0 ? 'font-strong text-[var(--red-ink)]' : 'text-tx-3')}
                      title={t('endpoints.columns.error_rate')}
                    >
                      {formatRate(item.red.errorRate)}
                    </span>
                    <span role="cell" className="text-right tabular-nums text-tx-1" title={t('endpoints.columns.p95')}>
                      {formatDuration(item.red.p95Time ?? undefined)}
                    </span>
                  </>
                ) : (
                  <span role="cell" className="col-span-3 text-right text-tx-3">
                    {t('settings.key_transactions.no_traffic')}
                  </span>
                )}
              </OpenRow>
            );
          })}
        </div>
      )}
    </ApmCard>
  );
}
