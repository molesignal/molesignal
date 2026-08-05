import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, XCircle } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as emailDomainsApi from '@/api/emailDomains';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { IamListPage } from './IamLayout';

export function EmailDomains() {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({
    permission: 'org.settings.manage',
  });
  const [draft, setDraft] = React.useState('');

  const domainsQuery = useQuery({
    queryKey: ['iam', 'email-domains'],
    queryFn: () => emailDomainsApi.list(),
  });

  const setData = (domains: string[]) =>
    qc.setQueryData(['iam', 'email-domains'], domains);

  const add = useMutation({
    mutationFn: (domain: string) => emailDomainsApi.add(domain),
    onSuccess: (domains) => {
      toast.success(t('email_domains.toast_added'));
      setData(domains);
      setDraft('');
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const remove = useMutation({
    mutationFn: (domain: string) => emailDomainsApi.remove(domain),
    onSuccess: (domains) => {
      toast.success(t('email_domains.toast_removed'));
      setData(domains);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const normalizedDraft = draft.trim().replace(/^[@.]+/, '');
  const invalid =
    !normalizedDraft.includes('.') ||
    /[@\s]/.test(normalizedDraft);
  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!manageAccess.disabled && !invalid && !add.isPending) {
      add.mutate(normalizedDraft);
    }
  };

  const rows = (domainsQuery.data ?? []).map((domain) => ({ domain }));

  const state = productStateFor(
    queryStateFor({
      isLoading: domainsQuery.isLoading,
      isError: domainsQuery.isError,
      data: rows,
    }),
    {
      error: domainsQuery.error,
      emptyTitle: t('email_domains.empty_title'),
      emptyDescription: t('email_domains.empty_description'),
    },
  );

  return (
    <IamListPage title={t('email_domains.title')} subtitle={t('email_domains.subtitle')}>
      <form onSubmit={submit} className="mb-4 flex items-center gap-2">
        <FormInput
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder={t('email_domains.placeholder')}
          aria-label={t('email_domains.add')}
          disabled={manageAccess.disabled || add.isPending}
          disabledReason={manageAccess.reason}
        />
        <ChromeButton
          type="submit"
          variant="primary"
          disabled={manageAccess.disabled || invalid || add.isPending}
          disabledReason={
            manageAccess.reason ??
            (invalid ? tc('access.form_invalid') : undefined)
          }
        >
          <Plus className="h-3 w-3" /> {t('email_domains.add')}
        </ChromeButton>
      </form>
      {state ? (
        <ProductState {...state} />
      ) : (
        <DataTable
          rows={rows}
          rowKey={(row) => row.domain}
          columns={[
            {
              key: 'domain',
              header: t('email_domains.columns.domain'),
              cell: (row) => <span className="text-tx-0">{row.domain}</span>,
            },
            {
              key: 'actions',
              header: t('email_domains.columns.actions'),
              width: 100,
              cell: (row) => (
                <IconButton
                  disabled={manageAccess.disabled || remove.isPending}
                  disabledReason={
                    manageAccess.reason ??
                    (remove.isPending
                      ? tc('access.operation_pending')
                      : undefined)
                  }
                  onClick={() => remove.mutate(row.domain)}
                  className="enabled:hover:bg-red-dim enabled:hover:text-red-soft"
                  aria-label={t('email_domains.remove')}
                >
                  <XCircle className="h-3 w-3" />
                </IconButton>
              ),
            },
          ]}
        />
      )}
    </IamListPage>
  );
}
