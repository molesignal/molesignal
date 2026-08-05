import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as functionsApi from '@/api/functions';
import { useActionAccess } from '@/product/actionAccess';
import { type ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton, Pill } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { FormDrawer } from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { formatMicros } from '../rum/_helpers';

export function FunctionsList() {
  const { t } = useTranslation('functions');
  const navigate = useNavigate();
  const createAccess = useActionAccess({
    permission: 'functions.create',
  });
  const [search, setSearch] = React.useState('');
  const [viewing, setViewing] = React.useState<functionsApi.FunctionResp | null>(null);

  // functions.list 现同时返回 org 自有函数 + 全局内置预设（org `__builtin__`，is_builtin=true）。
  const q = useQuery({
    queryKey: ['functions', 'list'],
    queryFn: () => functionsApi.list(),
  });

  const rows = React.useMemo(() => {
    const data = q.data ?? [];
    if (!search) return data;
    const needle = search.toLowerCase();
    return data.filter((r) => r.name.toLowerCase().includes(needle));
  }, [q.data, search]);

  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const listState: ProductStateProps | null =
    state === 'loading'
      ? { variant: 'loading' }
      : state === 'error'
        ? { variant: 'error', error: q.error }
        : state === 'empty'
          ? {
              variant: 'empty',
              title: t('list.empty_title'),
              description: t('list.empty_description'),
              action: (
                <ChromeButton
                  variant="primary"
                  disabled={createAccess.disabled}
                  disabledReason={createAccess.reason}
                  onClick={() =>
                    createAccess.allowed && navigate('/functions/new')
                  }
                >
                  {t('list.new_function')}
                </ChromeButton>
              ),
            }
          : null;

  return (
    <>
      <ListPage
        title={t('title')}
        subtitle={t('subtitle') as string}
        toolbar={
          <ChromeButton
            variant="primary"
            disabled={createAccess.disabled}
            disabledReason={createAccess.reason}
            onClick={() => createAccess.allowed && navigate('/functions/new')}
          >
            {t('list.new_function')}
          </ChromeButton>
        }
        filters={
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t('list.search_placeholder') ?? ''}
            className="h-8 w-full max-w-[280px] rounded-md border border-bd-1 bg-bg-2 px-2.5 font-sans text-xs text-tx-0 placeholder:text-tx-3 focus:outline-none"
          />
        }
        state={listState}
      >
        <DataTable
          rows={rows}
          rowKey={(r) => r.id}
          onRowClick={(r) =>
            r.is_builtin ? setViewing(r) : navigate(`/functions/${encodeURIComponent(r.id)}`)
          }
          columns={[
            {
              key: 'name',
              header: t('list.columns.name'),
              cell: (r) => (
                <div className="flex flex-col">
                  <span className="font-sans text-tx-0">{r.name}</span>
                  {r.is_builtin && r.description ? (
                    <span className="font-sans text-xs text-tx-3">{r.description}</span>
                  ) : null}
                </div>
              ),
            },
            {
              key: 'lang',
              header: t('list.columns.language'),
              cell: (r) => (
                <div className="flex items-center gap-1.5">
                  <Pill tone={r.language === 'vrl' ? 'orange' : 'blue'}>{r.language}</Pill>
                  {r.is_builtin ? <Pill tone="dim">{t('list.builtin')}</Pill> : null}
                </div>
              ),
              width: 160,
            },
            {
              key: 'updated',
              header: t('list.columns.updated'),
              cell: (r) => (r.is_builtin ? '—' : formatMicros(r.updated_at_micros)),
              width: 200,
            },
          ]}
        />
      </ListPage>
      <FormDrawer
        open={viewing !== null}
        onOpenChange={(open) => !open && setViewing(null)}
        title={viewing?.name ?? ''}
        subtitle={t('list.builtin')}
        footer={
          <div className="flex justify-end gap-2">
            <CopyIconButton
              label={t('list.preset_copy')}
              onClick={() => {
                if (!viewing) return;
                void navigator.clipboard?.writeText(viewing.source);
                toast.success(t('list.preset_copied'));
              }}
            />
            <ChromeButton variant="primary" onClick={() => setViewing(null)}>
              {t('list.preset_close')}
            </ChromeButton>
          </div>
        }
      >
        {viewing?.description ? (
          <p className="mb-3 font-sans text-xs text-tx-2">{viewing.description}</p>
        ) : null}
        <pre className="max-h-[60vh] overflow-auto rounded-md border border-bd-1 bg-bg-2 p-3 font-mono text-xs leading-relaxed text-tx-1">
          {viewing?.source}
        </pre>
      </FormDrawer>
    </>
  );
}
