import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { apmApi, apmQueryKeys, type ServiceDefinition, type TransactionIdentity } from '@/api/apm';
import { apmCatalogApi } from '@/api/apmCatalog';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { toast } from '@/shell/ui/sonner';

import { serviceKey } from '../scope';

/** An endpoint as key transactions name it: its kind and its name. */
export function transactionKey(transaction: TransactionIdentity): string {
  return `${transaction.kind}:${transaction.name}`;
}

/** The definitions with the one of `saved`'s service in its place, or added. */
export function withDefinition(definitions: readonly ServiceDefinition[] | undefined, saved: ServiceDefinition): ServiceDefinition[] {
  const key = serviceKey(saved);
  const rest = (definitions ?? []).filter((definition) => serviceKey(definition) !== key);
  return [...rest, saved];
}

/**
 * The key transactions of a service: which endpoints are starred, and the
 * star set or taken off one endpoint at a time, at once on the page and a
 * moment later on the server. Everyone sees the stars; those who may manage
 * APM change them.
 */
export function useKeyTransactions(service: { orgId: string; namespace: string; name: string }) {
  const { t } = useTranslation('apm');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'apm.definitions.manage' });
  const queryKey = apmQueryKeys.definitions(service.orgId);
  const definitions = useQuery({ queryKey, queryFn: apmApi.definitions, enabled: Boolean(service.orgId) });
  const definition = definitions.data?.find((entry) => serviceKey(entry) === serviceKey(service)) ?? null;
  const keys = React.useMemo(() => new Set((definition?.keyTransactions ?? []).map(transactionKey)), [definition]);

  const mark = useMutation({
    mutationFn: ({ transaction, key }: { transaction: TransactionIdentity; key: boolean }) =>
      apmCatalogApi.markKeyTransaction({ namespace: service.namespace, name: service.name }, transaction, key),
    onMutate: async ({ transaction, key }) => {
      await queryClient.cancelQueries({ queryKey });
      const before = queryClient.getQueryData<ServiceDefinition[]>(queryKey);
      const current = before?.find((entry) => serviceKey(entry) === serviceKey(service));

      if (current) {
        const others = (current.keyTransactions ?? []).filter((kept) => transactionKey(kept) !== transactionKey(transaction));
        queryClient.setQueryData(queryKey, withDefinition(before, { ...current, keyTransactions: key ? [...others, transaction] : others }));
      }

      return { before };
    },
    onSuccess: (saved) => queryClient.setQueryData<ServiceDefinition[]>(queryKey, (current) => withDefinition(current, saved)),
    onError: (error, _change, context) => {
      if (context?.before) queryClient.setQueryData(queryKey, context.before);
      toast.error(toApiError(error).message);
    },
  });

  return {
    definition,
    keys,
    isKey: (transaction: TransactionIdentity) => keys.has(transactionKey(transaction)),
    toggle: (transaction: TransactionIdentity) => mark.mutate({ transaction, key: !keys.has(transactionKey(transaction)) }),
    canChange: access.allowed,
    /** Why the star cannot be changed, or what it does when it can. */
    title: (transaction: TransactionIdentity) =>
      access.reason ?? t(keys.has(transactionKey(transaction)) ? 'endpoints.unmark_key' : 'endpoints.mark_key', { name: transaction.name }),
  };
}
