import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useSearchParams } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as statusPagesApi from '@/api/statusPages';
import type { StatusPage, StatusPageLifecycle } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { toast } from '@/shell/ui/sonner';

import { PageFormDrawer, type PageFormSubmission } from '../ConfigurationDrawers';
import { StatusPagesActivation } from './Activation';
import { StatusPageRow } from './PageRow';

type PendingAction = { kind: 'archive' | 'delete'; page: StatusPage } | null;

/**
 * Every status page of the workspace, each as what its readers see on it now.
 * A workspace has a handful of them, so they are rows to read, not a table to
 * scan: the name, the verdict, what is under way, how the components divide.
 */
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
  const setLifecycle = (next: StatusPageLifecycle) => {
    setSearchParams(next === 'archived' ? { lifecycle: 'archived' } : {}, { replace: true });
  };
  const openCreate = () => manageAccess.allowed && setCreating(true);
  // Before the first page there is nothing to list: the module says what a status page is instead.
  const firstRun = lifecycle === 'active' && pagesQuery.isSuccess && pages.length === 0;
  const state: ProductStateProps | null = pagesQuery.isLoading
    ? { variant: 'loading' }
    : pagesQuery.isError
      ? { variant: 'error', error: pagesQuery.error }
      : pages.length === 0
        ? {
            variant: 'empty',
            title: t('states.no_archived_pages'),
            description: t('states.no_archived_pages_description'),
          }
        : null;

  return (
    <>
      <ListPage
        appearance="surface"
        title={t('title')}
        subtitle={t('subtitle')}
        // The first run carries the one way forward itself; the header does not offer it again.
        toolbar={
          firstRun ? undefined : (
            <ChromeButton
              variant="primary"
              disabled={manageAccess.disabled || lifecycle === 'archived'}
              disabledReason={manageAccess.reason}
              onClick={openCreate}
            >
              <Plus className="h-3.5 w-3.5" />
              {t('actions.create_page')}
            </ChromeButton>
          )
        }
        {...(firstRun
          ? { contentSurface: false }
          : {
              listHead: (
                <ViewTabs
                  label={t('lifecycle_label')}
                  value={lifecycle}
                  onValueChange={setLifecycle}
                  items={(['active', 'archived'] as const).map((item) => ({
                    key: item,
                    label: t(`lifecycle.${item}`),
                  }))}
                />
              ),
              state,
            })}
      >
        {firstRun ? (
          <StatusPagesActivation
            manageAccess={manageAccess}
            onCreate={openCreate}
            onShowArchived={() => setLifecycle('archived')}
          />
        ) : (
          <ul aria-label={t('title')} className="divide-y divide-bd-0">
            {pages.map((page) => (
              <StatusPageRow
                key={page.id}
                page={page}
                manageAccess={manageAccess}
                restoring={restoreMutation.isPending}
                onArchive={() => manageAccess.allowed && setPendingAction({ kind: 'archive', page })}
                onRestore={() => manageAccess.allowed && restoreMutation.mutate(page.id)}
                onDelete={() => manageAccess.allowed && setPendingAction({ kind: 'delete', page })}
              />
            ))}
          </ul>
        )}
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
