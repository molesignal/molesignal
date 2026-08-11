import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Archive, ArchiveRestore, ExternalLink, Pencil, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useSearchParams } from 'react-router-dom';

import { ConfirmDialog, DataTable } from '@/admin';
import * as statusPagesApi from '@/api/statusPages';
import type { StatusPage, StatusPageLifecycle } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';

import { PageFormDrawer, type PageFormSubmission } from './ConfigurationDrawers';
import { statusPageLanguageLabel, statusPageLanguages } from './model';
import { publicStatusPageUrl } from './publicUrl';

type PendingAction = { kind: 'archive' | 'delete'; page: StatusPage } | null;

export function StatusPages() {
  const { t } = useTranslation('status-pages');
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'status_pages.manage' });
  const [searchParams, setSearchParams] = useSearchParams();
  const lifecycle: StatusPageLifecycle =
    searchParams.get('lifecycle') === 'archived' ? 'archived' : 'active';
  const [creating, setCreating] = React.useState(false);
  const [pendingAction, setPendingAction] = React.useState<PendingAction>(null);
  const pagesQuery = useQuery({
    queryKey: ['status-pages', lifecycle],
    queryFn: () => statusPagesApi.list(lifecycle),
  });

  const refresh = () => queryClient.invalidateQueries({ queryKey: ['status-pages'] });
  const createMutation = useMutation({
    mutationFn: async ({ input, logoFile }: PageFormSubmission) => {
      let page = await statusPagesApi.create(input);
      if (!logoFile) return page;
      try {
        page = await statusPagesApi.uploadLogo(page.id, logoFile);
      } catch (error) {
        toast.error(t('toast.logo_upload_failed', { error: toApiError(error).message }));
      }
      return page;
    },
    onSuccess: (page) => {
      toast.success(t('toast.page_created'));
      setCreating(false);
      void refresh();
      navigate(`/status-pages/${page.id}/overview`);
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const archiveMutation = useMutation({
    mutationFn: (pageId: string) => statusPagesApi.archive(pageId),
    onSuccess: () => {
      toast.success(t('toast.page_archived'));
      setPendingAction(null);
      void refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const restoreMutation = useMutation({
    mutationFn: (pageId: string) => statusPagesApi.restore(pageId),
    onSuccess: () => {
      toast.success(t('toast.page_restored'));
      void refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const deleteMutation = useMutation({
    mutationFn: (pageId: string) => statusPagesApi.remove(pageId),
    onSuccess: () => {
      toast.success(t('toast.page_deleted'));
      setPendingAction(null);
      void refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  const pages = pagesQuery.data ?? [];
  const state: ProductStateProps | null = pagesQuery.isLoading
    ? { variant: 'loading' }
    : pagesQuery.isError
      ? { variant: 'error', error: pagesQuery.error }
      : pages.length === 0
        ? {
            variant: 'empty',
            title: t(lifecycle === 'active' ? 'states.empty_title' : 'states.no_archived_pages'),
            description: t(
              lifecycle === 'active' ? 'states.empty_description' : 'states.no_archived_pages_description',
            ),
            action:
              lifecycle === 'active' ? (
                <ChromeButton
                  variant="primary"
                  disabled={manageAccess.disabled}
                  disabledReason={manageAccess.reason}
                  onClick={() => manageAccess.allowed && setCreating(true)}
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t('actions.create_page')}
                </ChromeButton>
              ) : undefined,
          }
        : null;

  const setLifecycle = (next: StatusPageLifecycle) => {
    setSearchParams(next === 'archived' ? { lifecycle: 'archived' } : {}, { replace: true });
  };

  return (
    <>
      <ListPage
        title={t('title')}
        subtitle={t('subtitle')}
        toolbar={
          <ChromeButton
            variant="primary"
            disabled={manageAccess.disabled || lifecycle === 'archived'}
            disabledReason={manageAccess.reason}
            onClick={() => manageAccess.allowed && setCreating(true)}
          >
            <Plus className="h-3.5 w-3.5" />
            {t('actions.create_page')}
          </ChromeButton>
        }
        filters={
          <div className="flex items-center gap-1">
            {(['active', 'archived'] as const).map((item) => (
              <ChromeButton
                key={item}
                size="sm"
                variant={lifecycle === item ? 'primary' : 'ghost'}
                onClick={() => setLifecycle(item)}
              >
                {t(`lifecycle.${item}`)}
              </ChromeButton>
            ))}
          </div>
        }
        kpis={[
          { label: t('kpis.pages'), value: String(pages.length) },
          { label: t('kpis.public'), value: String(pages.filter((page) => page.visibility === 'public').length) },
          { label: t('kpis.private'), value: String(pages.filter((page) => page.visibility === 'private').length) },
        ]}
        kpiLayout="inline"
        state={state}
      >
        <DataTable
          rows={pages}
          rowKey={(page) => page.id}
          onRowClick={(page) => navigate(`/status-pages/${page.id}/overview`)}
          columns={[
            {
              key: 'name',
              header: t('columns.name'),
              cell: (page) => (
                <div className="min-w-0">
                  <div className="truncate text-tx-0">{page.name}</div>
                  <div className="mt-0.5 truncate font-mono text-xs text-tx-3">
                    {publicStatusPageUrl(page)}
                  </div>
                </div>
              ),
            },
            {
              key: 'visibility',
              header: t('columns.visibility'),
              width: 120,
              cell: (page) => (
                <Pill tone={page.visibility === 'public' ? 'green' : 'dim'}>
                  {t(`visibility.${page.visibility}`)}
                </Pill>
              ),
            },
            {
              key: 'language',
              header: t('columns.language'),
              width: 180,
              cell: (page) => statusPageLanguages(page.language, page.languages)
                .map(statusPageLanguageLabel)
                .join(' · '),
            },
            {
              key: 'updated',
              header: t('columns.updated'),
              width: 170,
              cell: (page) => (
                <span className="tabular-nums text-tx-3">{formatMicrosActive(page.updated_at)}</span>
              ),
            },
            {
              key: 'actions',
              header: t('columns.actions'),
              width: lifecycle === 'active' ? 120 : 150,
              cell: (page) => (
                <div className="flex items-center gap-0.5" onClick={(event) => event.stopPropagation()}>
                  {lifecycle === 'active' && (
                    <IconButton
                      aria-label={t('actions.open_public')}
                      title={t('actions.open_public')}
                      onClick={() => window.open(publicStatusPageUrl(page), '_blank', 'noopener,noreferrer')}
                    >
                      <ExternalLink className="h-3.5 w-3.5" />
                    </IconButton>
                  )}
                  <IconButton
                    aria-label={t('actions.manage')}
                    title={t('actions.manage')}
                    onClick={() => navigate(`/status-pages/${page.id}/overview`)}
                  >
                    <Pencil className="h-3.5 w-3.5" />
                  </IconButton>
                  {lifecycle === 'active' ? (
                    <IconButton
                      aria-label={t('actions.archive')}
                      title={t('actions.archive')}
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      onClick={() => manageAccess.allowed && setPendingAction({ kind: 'archive', page })}
                    >
                      <Archive className="h-3.5 w-3.5" />
                    </IconButton>
                  ) : (
                    <>
                      <IconButton
                        aria-label={t('actions.restore')}
                        title={t('actions.restore')}
                        disabled={manageAccess.disabled || restoreMutation.isPending}
                        disabledReason={manageAccess.reason}
                        onClick={() => manageAccess.allowed && restoreMutation.mutate(page.id)}
                      >
                        <ArchiveRestore className="h-3.5 w-3.5" />
                      </IconButton>
                      <IconButton
                        aria-label={t('actions.delete')}
                        title={t('actions.delete')}
                        disabled={manageAccess.disabled}
                        disabledReason={manageAccess.reason}
                        className="enabled:hover:text-red-soft"
                        onClick={() => manageAccess.allowed && setPendingAction({ kind: 'delete', page })}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </IconButton>
                    </>
                  )}
                </div>
              ),
            },
          ]}
        />
      </ListPage>

      <PageFormDrawer
        open={creating}
        page={null}
        busy={createMutation.isPending}
        onOpenChange={setCreating}
        onSubmit={(submission) => createMutation.mutate(submission)}
      />
      <ConfirmDialog
        open={Boolean(pendingAction)}
        onOpenChange={(open) => !open && setPendingAction(null)}
        title={t(`confirm.${pendingAction?.kind ?? 'archive'}_page_title`)}
        description={t(`confirm.${pendingAction?.kind ?? 'archive'}_page_description`, {
          name: pendingAction?.page.name,
        })}
        confirmLabel={t(`actions.${pendingAction?.kind ?? 'archive'}`)}
        destructive={pendingAction?.kind === 'delete'}
        busy={archiveMutation.isPending || deleteMutation.isPending}
        onConfirm={() => {
          if (!pendingAction) return;
          if (pendingAction.kind === 'archive') archiveMutation.mutate(pendingAction.page.id);
          else deleteMutation.mutate(pendingAction.page.id);
        }}
      />
    </>
  );
}
