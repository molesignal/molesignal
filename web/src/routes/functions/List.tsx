import { useQuery } from '@tanstack/react-query';
import { Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as functionsApi from '@/api/functions';
import { useActionAccess } from '@/product/actionAccess';
import { type ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton, Pill } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import { ViewTabs } from '@/shell/tabs/ViewTabs';

import { formatMicros } from '../rum/_helpers';
import {
  FUNCTION_VIEWS,
  type FunctionView,
  languageName,
  startFromPath,
  viewCounts,
  visibleFunctions,
} from './list/model';
import { PresetDrawer } from './list/PresetDrawer';

/**
 * Every function a pipeline can call: the ones the workspace wrote and the presets the product
 * ships with. A preset is read-only; it is read in a drawer and can be the start of a function
 * of the workspace's own.
 */
export function FunctionsList() {
  const { t } = useTranslation('functions');
  const navigate = useNavigate();
  const createAccess = useActionAccess({
    permission: 'functions.create',
  });
  const [search, setSearch] = React.useState('');
  const [view, setView] = React.useState<FunctionView>('all');
  const [viewing, setViewing] = React.useState<functionsApi.FunctionResp | null>(null);

  // functions.list 现同时返回 org 自有函数 + 全局内置预设（org `__builtin__`，is_builtin=true）。
  const q = useQuery({
    queryKey: ['functions', 'list'],
    queryFn: () => functionsApi.list(),
  });

  const functions = React.useMemo(() => q.data ?? [], [q.data]);
  const counts = React.useMemo(() => viewCounts(functions), [functions]);
  const rows = React.useMemo(
    () => visibleFunctions(functions, view, search),
    [functions, view, search],
  );

  const startFrom = (preset: functionsApi.FunctionResp) => {
    if (createAccess.allowed) navigate(startFromPath(preset));
  };

  const newFunction = (
    <ChromeButton
      variant="primary"
      disabled={createAccess.disabled}
      disabledReason={createAccess.reason}
      onClick={() => createAccess.allowed && navigate('/functions/new')}
    >
      {t('list.new_function')}
    </ChromeButton>
  );

  const state: ProductStateProps | null = q.isLoading
    ? { variant: 'loading' }
    : q.isError
      ? { variant: 'error', error: q.error }
      : rows.length > 0
        ? null
        : search.trim()
          ? {
              variant: 'empty',
              compact: true,
              title: t('list.no_match_title'),
              description: t('list.no_match_description'),
              action: <ChromeButton onClick={() => setSearch('')}>{t('list.clear_search')}</ChromeButton>,
            }
          : view === 'builtin'
            ? { variant: 'empty', compact: true, title: t('list.no_presets_title') }
            : {
                variant: 'empty',
                title: t('list.empty_title'),
                description: t(
                  counts.builtin > 0 ? 'list.empty_description_presets' : 'list.empty_description',
                ),
                // Writing one is the page's own action, a line above: it is not offered twice.
                action:
                  counts.builtin > 0 && view !== 'all' ? (
                    <ChromeButton onClick={() => setView('builtin')}>{t('list.see_presets')}</ChromeButton>
                  ) : undefined,
              };

  return (
    <>
      <ListPage
        appearance="surface"
        title={t('title')}
        subtitle={t('subtitle') as string}
        toolbar={newFunction}
        state={state}
        listHead={
          // Nothing to search or to tell apart while the list is being read, or could not be.
          q.isSuccess ? (
            <>
              <label className="relative w-full min-w-0 sm:w-[300px]">
                <span className="sr-only">{t('list.search_label')}</span>
                <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
                <FormInput
                  data-function-search=""
                  value={search}
                  onChange={(event) => setSearch(event.target.value)}
                  placeholder={t('list.search_placeholder')}
                  className="pl-9"
                />
              </label>
              <ViewTabs<FunctionView>
                label={t('list.views_label')}
                value={view}
                onValueChange={setView}
                dataAttributes={{ 'data-function-views': '' }}
                items={FUNCTION_VIEWS.map((item) => ({
                  key: item,
                  label: t(`list.views.${item}`),
                  count: counts[item],
                }))}
              />
            </>
          ) : undefined
        }
      >
        <DataTable
          className="rounded-none border-0 bg-transparent"
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
                <div data-function-row={r.name} className="min-w-0 py-1">
                  <div className="flex min-w-0 items-center gap-2">
                    <span className="truncate font-strong text-tx-0">{r.name}</span>
                    {r.is_builtin && <Pill tone="dim">{t('list.builtin')}</Pill>}
                  </div>
                  {r.description ? (
                    <div className="mt-0.5 truncate text-xs text-tx-2">{r.description}</div>
                  ) : null}
                </div>
              ),
            },
            {
              key: 'lang',
              header: t('list.columns.language'),
              cell: (r) => languageName(r.language),
              width: 140,
            },
            {
              key: 'updated',
              header: t('list.columns.updated'),
              // A preset came with the product: it has no time of its own to tell.
              cell: (r) =>
                r.is_builtin ? (
                  <span className="text-tx-3">{t('list.ships_with_product')}</span>
                ) : (
                  formatMicros(r.updated_at_micros)
                ),
              width: 200,
            },
            {
              key: 'action',
              header: <span className="sr-only">{t('list.columns.action')}</span>,
              cell: (r) =>
                r.is_builtin ? (
                  // The row opens the preset; the button starts a function from it. Neither a
                  // click on the button nor a key pressed on it is the row's.
                  <div className="flex justify-end" onKeyDown={(event) => event.stopPropagation()}>
                    <ChromeButton
                      size="sm"
                      data-function-start-from={r.name}
                      disabled={createAccess.disabled}
                      disabledReason={createAccess.reason}
                      onClick={(event) => {
                        event.stopPropagation();
                        startFrom(r);
                      }}
                    >
                      {t('list.start_from')}
                    </ChromeButton>
                  </div>
                ) : null,
              width: 150,
            },
          ]}
        />
      </ListPage>
      <PresetDrawer
        preset={viewing}
        createAccess={createAccess}
        onClose={() => setViewing(null)}
        onStartFrom={startFrom}
      />
    </>
  );
}
