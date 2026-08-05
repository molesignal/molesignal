import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { type TFunction } from 'i18next';
import {
  Copy,
  Download,
  Ellipsis,
  Eye,
  Folder,
  FolderOpen,
  Loader2,
  Maximize2,
  Pencil,
  Plus,
  Search,
  Share2,
  Star,
  Trash2,
  Upload,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { ConfirmDialog, DataTable } from '@/admin';
import * as dashboardsApi from '@/api/dashboards';
import * as foldersApi from '@/api/folders';
import {
  dashboardDefinitionFromApi,
  flattenElements,
} from '@/dashboard-engine/model';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { type ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ResourceShareDialog } from '@/sharing/ResourceShareDialog';
import { ChromeButton, Pill } from '@/shell/chrome';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { setFullscreenDashboard } from '@/shell/wallboard';
import { useAuthStore } from '@/stores/auth';
import type { Dashboard as BackendDashboard } from '@/types/dashboard';

import { DashboardFolderManager } from './DashboardFolderManager';
import { DashboardHierarchyResource } from './DashboardHierarchyResource';
import {
  ALL_FOLDERS,
  DEFAULT_FOLDER,
  type FolderSummary,
  folderPath,
} from './folderModel';
import { buildDashboardHierarchy } from './hierarchy';

interface DisplayDashboard {
  id: string;
  name: string;
  folderId: string | undefined;
  folder: string;
  starred?: boolean;
  tags: string[];
  panels: number;
  edited: string;
  updatedAt: number;
  createdBy: string;
  updatedBy: string;
  source: BackendDashboard;
}

type DashboardScope = 'all' | 'mine' | 'favorites' | 'tag';

function adaptDashboard(
  d: BackendDashboard,
  opts: {
    defaultFolder: string;
    folderNameById: Map<string, string>;
    formatRelative: (value: number | undefined) => string;
  },
): DisplayDashboard {
  const panels = flattenElements(
    dashboardDefinitionFromApi(d).elements,
  ).filter(
    (element) => element.kind === 'panel' || element.kind === 'text',
  ).length;
  const folderId = d.folder_id;
  return {
    id: d.id,
    name: d.title,
    folderId,
    folder: folderId ? (opts.folderNameById.get(folderId) ?? folderId) : opts.defaultFolder,
    tags: d.tags ?? [],
    panels,
    edited: opts.formatRelative(d.updated_at ?? d.created_at),
    updatedAt: d.updated_at ?? d.created_at,
    createdBy: d.created_by ?? d.org_id,
    updatedBy: d.updated_by ?? d.created_by ?? d.org_id,
    source: d,
  };
}

function favoriteStorageKey(orgId: string): string {
  return `molesignal:dashboard-favorites:${orgId || 'default'}`;
}

function readFavoriteIds(orgId: string): Set<string> {
  if (typeof window === 'undefined') return new Set();
  try {
    const value = JSON.parse(window.localStorage.getItem(favoriteStorageKey(orgId)) ?? '[]');
    return new Set(Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);
  } catch {
    return new Set();
  }
}

function formatRelative(
  microsOrSeconds: number | undefined,
  t: TFunction<'dashboards'>,
): string {
  if (!microsOrSeconds) return t('relative.unknown');
  // Backend uses TimestampMicros (microseconds since epoch). Anything < 1e12
  // is plausibly seconds; otherwise treat as micros.
  const ms = microsOrSeconds > 1e12 ? Math.floor(microsOrSeconds / 1000) : microsOrSeconds * 1000;
  const diff = Date.now() - ms;
  if (diff < 60_000) return t('relative.just_now');
  if (diff < 3_600_000) return t('relative.minutes_ago', { count: Math.round(diff / 60_000) });
  if (diff < 86_400_000) return t('relative.hours_ago', { count: Math.round(diff / 3_600_000) });
  return t('relative.days_ago', { count: Math.round(diff / 86_400_000) });
}

export function Dashboards() {
  const { t } = useTranslation('dashboards');
  const [search, setSearch] = React.useState('');
  const [folder, setFolder] = React.useState(ALL_FOLDERS);
  const [scope, setScope] = React.useState<DashboardScope>('all');
  const [selectedTag, setSelectedTag] = React.useState('');
  const [foldersOpen, setFoldersOpen] = React.useState(false);
  const [collapsedFolderIds, setCollapsedFolderIds] =
    React.useState<Set<string>>(() => new Set());
  const [deleteTarget, setDeleteTarget] = React.useState<DisplayDashboard | null>(null);
  const [sharingDashboard, setSharingDashboard] =
    React.useState<DisplayDashboard | null>(null);
  const nav = useNavigate();
  const qc = useQueryClient();
  const createAccess = useActionAccess({ permission: 'dashboards.create' });
  const editAccess = useActionAccess({ permission: 'dashboards.edit' });
  const deleteAccess = useActionAccess({ permission: 'dashboards.delete' });
  const shareAccess = useActionAccess({ permission: 'dashboards.share' });
  const auth = useAuthStore((state) => state.ctx);
  const orgId = auth?.org_id ?? '';
  const userId = auth?.user_id ?? '';
  const [favoriteIds, setFavoriteIds] = React.useState<Set<string>>(() => readFavoriteIds(orgId));

  React.useEffect(() => {
    setFavoriteIds(readFavoriteIds(orgId));
  }, [orgId]);

  const listQuery = useQuery({
    queryKey: ['dashboards', 'list'],
    queryFn: () => dashboardsApi.list(),
  });

  const foldersQuery = useQuery({
    queryKey: ['folders', 'list'],
    queryFn: () => foldersApi.list(),
  });

  const apiFolders = React.useMemo(() => foldersQuery.data ?? [], [foldersQuery.data]);
  const folderNameById = React.useMemo(
    () => new Map(apiFolders.map((folder) => [folder.id, folder.name])),
    [apiFolders],
  );

  const { dashboards: all, invalidCount } = React.useMemo(
    () => {
      const dashboards: DisplayDashboard[] = [];
      let invalidCount = 0;
      for (const dashboard of listQuery.data ?? []) {
        try {
          dashboards.push({
            ...adaptDashboard(dashboard, {
              defaultFolder: t('list.default_folder'),
              folderNameById,
              formatRelative: (value) => formatRelative(value, t),
            }),
            starred: favoriteIds.has(dashboard.id),
          });
        } catch {
          invalidCount += 1;
        }
      }
      return { dashboards, invalidCount };
    },
    [favoriteIds, folderNameById, listQuery.data, t],
  );

  const folderSummaries = React.useMemo(() => {
    const summaries = new Map<string, FolderSummary>();
    for (const apiFolder of apiFolders) {
      summaries.set(apiFolder.id, {
        id: apiFolder.id,
        name: apiFolder.name,
        parentId: apiFolder.parent_id,
        dashboards: 0,
        panels: 0,
        managed: true,
        depth: 0,
      });
    }
    summaries.set(DEFAULT_FOLDER, {
      id: DEFAULT_FOLDER,
      name: t('list.default_folder'),
      parentId: undefined,
      dashboards: 0,
      panels: 0,
      managed: false,
      depth: 0,
    });
    for (const dashboard of all) {
      const folderId = dashboard.folderId ?? DEFAULT_FOLDER;
      const current = summaries.get(folderId) ?? {
        id: folderId,
        name: dashboard.folder,
        parentId: undefined,
        dashboards: 0,
        panels: 0,
        managed: false,
        depth: 0,
      };
      current.dashboards += 1;
      current.panels += dashboard.panels;
      summaries.set(folderId, current);
    }
    const byId = new Map(summaries);
    for (const summary of summaries.values()) {
      let depth = 0;
      let parentId = summary.parentId;
      while (parentId && byId.has(parentId) && depth < summaries.size + 1) {
        depth += 1;
        parentId = byId.get(parentId)?.parentId;
      }
      summary.depth = depth;
    }
    return Array.from(summaries.values()).sort((a, b) => {
      if (a.id === DEFAULT_FOLDER) return -1;
      if (b.id === DEFAULT_FOLDER) return 1;
      return folderPath(a, byId).localeCompare(folderPath(b, byId));
    });
  }, [all, apiFolders, t]);

  const tags = React.useMemo(
    () => Array.from(new Set(all.flatMap((dashboard) => dashboard.tags))).sort((a, b) => a.localeCompare(b)),
    [all],
  );
  const folderSummaryById = React.useMemo(
    () => new Map(folderSummaries.map((item) => [item.id, item])),
    [folderSummaries],
  );
  const folderSearchTextById = React.useMemo(
    () =>
      new Map(
        folderSummaries.map((item) => [
          item.id,
          folderPath(item, folderSummaryById).toLowerCase(),
        ]),
      ),
    [folderSummaries, folderSummaryById],
  );
  const expandableFolderIds = React.useMemo(() => {
    const ids = new Set<string>();
    for (const summary of folderSummaries) {
      if (summary.dashboards > 0) ids.add(summary.id);
      if (summary.parentId) ids.add(summary.parentId);
    }
    return ids;
  }, [folderSummaries]);
  const normalizedSearch = search.trim().toLowerCase();
  const effectiveCollapsedFolderIds =
    normalizedSearch === '' ? collapsedFolderIds : new Set<string>();
  const filtered = all.filter((dashboard) => {
    if (scope === 'mine' && dashboard.createdBy !== userId && dashboard.updatedBy !== userId) return false;
    if (scope === 'favorites' && !dashboard.starred) return false;
    if (scope === 'tag' && selectedTag && !dashboard.tags.includes(selectedTag)) return false;
    return (
      normalizedSearch === '' ||
      dashboard.name.toLowerCase().includes(normalizedSearch) ||
      dashboard.tags.some((tag) => tag.toLowerCase().includes(normalizedSearch)) ||
      (folderSearchTextById.get(dashboard.folderId ?? DEFAULT_FOLDER) ?? '')
        .includes(normalizedSearch)
    );
  });
  const searchMatchedFolderIds = new Set(
    normalizedSearch === ''
      ? []
      : folderSummaries
          .filter((item) =>
            (folderSearchTextById.get(item.id) ?? '').includes(normalizedSearch),
          )
          .map((item) => item.id),
  );
  const resources = buildDashboardHierarchy({
    folders: folderSummaries,
    dashboards: filtered,
    defaultFolderId: DEFAULT_FOLDER,
    rootFolderId: folder === ALL_FOLDERS ? undefined : folder,
    includeEmptyFolders: scope === 'all' && normalizedSearch === '',
    additionalVisibleFolderIds: searchMatchedFolderIds,
    collapsedFolderIds: effectiveCollapsedFolderIds,
  });
  const latestUpdated = all.reduce((latest, dashboard) => Math.max(latest, dashboard.updatedAt), 0);
  const summary = t(
    invalidCount > 0 ? 'list.summary_with_invalid' : 'list.summary',
    {
      dashboards: all.length,
      folders: folderSummaries.length,
      updated: formatRelative(latestUpdated, t),
      invalid: invalidCount,
    },
  );

  const listState: ProductStateProps | null =
    listQuery.isLoading
      ? { variant: 'loading' }
      : listQuery.isError
        ? { variant: 'error', error: listQuery.error }
        : all.length === 0
          ? {
              variant: 'empty',
              title: t(
                invalidCount > 0
                  ? 'list.invalid_only_title'
                  : 'list.empty_title',
              ),
              description: t(
                invalidCount > 0
                  ? 'list.invalid_only_description'
                  : 'list.empty_description',
                { count: invalidCount },
              ),
              action: (
                <ChromeButton
                  variant="primary"
                  disabled={createAccess.disabled}
                  disabledReason={createAccess.reason}
                  onClick={() => nav('/dashboards/new/edit')}
                >
                  <Plus className="h-3 w-3" /> {t('actions.new_dashboard')}
                </ChromeButton>
              ),
            }
          : resources.length === 0
            ? {
                variant: 'empty',
                title: t('list.no_results_title'),
                description: t('list.no_results_description'),
              }
          : null;

  const persistFavorites = (next: Set<string>) => {
    setFavoriteIds(next);
    if (typeof window !== 'undefined') {
      window.localStorage.setItem(favoriteStorageKey(orgId), JSON.stringify(Array.from(next)));
    }
  };

  const toggleFavorite = (dashboard: DisplayDashboard) => {
    const next = new Set(favoriteIds);
    if (next.has(dashboard.id)) next.delete(dashboard.id);
    else next.add(dashboard.id);
    persistFavorites(next);
  };

  const toggleFolderCollapsed = (folderId: string) => {
    setCollapsedFolderIds((current) => {
      const next = new Set(current);
      if (next.has(folderId)) next.delete(folderId);
      else next.add(folderId);
      return next;
    });
  };

  const refreshResources = async () => {
    await qc.invalidateQueries({ queryKey: ['dashboards', 'list'] });
  };

  const moveMutation = useMutation({
    mutationFn: ({ dashboard, folderId }: { dashboard: DisplayDashboard; folderId: string | undefined }) =>
      dashboardsApi.update(dashboard.id, dashboard.source.model, folderId),
    onSuccess: async (_saved, input) => {
      await refreshResources();
      toast.success(t('list.toast.moved', { title: input.dashboard.name }));
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  const duplicateMutation = useMutation({
    mutationFn: (dashboard: DisplayDashboard) =>
      dashboardsApi.create(
        {
          ...dashboard.source.model,
          uid: '',
          title: t('list.copy_name', { title: dashboard.name }),
        },
        dashboard.folderId,
      ),
    onSuccess: async (dashboard) => {
      await refreshResources();
      toast.success(t('list.toast.copied', { title: dashboard.title }));
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  const deleteMutation = useMutation({
    mutationFn: (dashboard: DisplayDashboard) => dashboardsApi.remove(dashboard.id),
    onSuccess: async (_data, dashboard) => {
      const nextFavorites = new Set(favoriteIds);
      nextFavorites.delete(dashboard.id);
      persistFavorites(nextFavorites);
      setDeleteTarget(null);
      await refreshResources();
      toast.success(t('list.toast.deleted', { title: dashboard.name }));
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  const exportDashboard = (dashboard: DisplayDashboard) => {
    const blob = new Blob([JSON.stringify(dashboard.source.model, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = `${dashboard.name.replace(/[^\p{L}\p{N}._-]+/gu, '-') || 'dashboard'}.json`;
    anchor.click();
    URL.revokeObjectURL(url);
  };

  const setAsNocDashboard = (dashboard: DisplayDashboard) => {
    if (!orgId) return;
    setFullscreenDashboard(orgId, {
      dashboardId: dashboard.id,
      title: dashboard.name,
      setAt: Date.now(),
    });
    toast.success(t('detail.fullscreen_saved', { title: dashboard.name }));
  };

  const actionProps = {
    folderOptions: folderSummaries,
    onDelete: setDeleteTarget,
    onDuplicate: (dashboard: DisplayDashboard) => duplicateMutation.mutate(dashboard),
    onEdit: (dashboard: DisplayDashboard) => nav(`/dashboards/${dashboard.id}/edit`),
    onExport: exportDashboard,
    onFavorite: toggleFavorite,
    onMove: (dashboard: DisplayDashboard, folderId: string | undefined) => moveMutation.mutate({ dashboard, folderId }),
    onOpen: (dashboard: DisplayDashboard) => nav(`/dashboards/${dashboard.id}`),
    onShare: (dashboard: DisplayDashboard) => setSharingDashboard(dashboard),
    createAccess,
    editAccess,
    deleteAccess,
    shareAccess,
    onSetNoc: setAsNocDashboard,
    sharingId: null,
    t,
  };

  return (
    <>
      <ListPage
        title={t('title')}
        subtitle={summary}
        toolbar={
          <>
            <ChromeButton
              disabled={createAccess.disabled}
              disabledReason={createAccess.reason}
              onClick={() => nav('/dashboards/import')}
            >
              <Upload className="h-3 w-3" /> {t('actions.import_json')}
            </ChromeButton>
            <ChromeButton onClick={() => setFoldersOpen(true)}>
              <FolderOpen className="h-3 w-3" /> {t('actions.manage_folders')}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              disabled={createAccess.disabled}
              disabledReason={createAccess.reason}
              onClick={() => nav('/dashboards/new/edit')}
            >
              <Plus className="h-3 w-3" /> {t('actions.new_dashboard')}
            </ChromeButton>
          </>
        }
        filters={
          <div className="flex w-full flex-wrap items-center gap-2">
            <div className="flex h-9 min-w-[240px] max-w-[420px] flex-1 items-center gap-2 rounded-md border border-bd-1 bg-bg-1 px-3 font-sans text-xs">
              <Search className="h-3 w-3 text-tx-3" />
              <input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={t('list.search_placeholder') ?? ''}
                className="min-w-0 flex-1 bg-transparent text-tx-0 placeholder:text-tx-3 focus:outline-none"
              />
            </div>
            <div className="flex gap-0.5 rounded-md border border-bd-0 bg-bg-1 p-0.5">
              {(['all', 'mine', 'favorites'] as const).map((candidate) => (
                <button
                  key={candidate}
                  type="button"
                  onClick={() => {
                    setScope(candidate);
                    setSelectedTag('');
                  }}
                  className={`rounded px-2.5 py-1 font-sans text-xs font-strong ${
                    scope === candidate ? 'bg-bg-4 text-tx-0' : 'text-tx-2 hover:text-tx-0'
                  }`}
                >
                  {t(`list.scopes.${candidate}`)}
                </button>
              ))}
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    type="button"
                    className={`rounded px-2.5 py-1 font-sans text-xs font-strong ${
                      scope === 'tag' ? 'bg-bg-4 text-tx-0' : 'text-tx-2 hover:text-tx-0'
                    }`}
                  >
                    {scope === 'tag' && selectedTag ? `#${selectedTag}` : t('list.scopes.tags')}
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start">
                  {tags.length === 0 ? (
                    <DropdownMenuItem disabled>{t('list.no_tags')}</DropdownMenuItem>
                  ) : tags.map((tag) => (
                    <DropdownMenuItem
                      key={tag}
                      onSelect={() => {
                        setScope('tag');
                        setSelectedTag(tag);
                      }}
                    >
                      #{tag}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
            {folder !== ALL_FOLDERS && (
              <button
                type="button"
                onClick={() => setFolder(ALL_FOLDERS)}
                className="inline-flex h-8 items-center gap-1.5 rounded-full border border-bd-0 bg-bg-1 px-3 font-sans text-xs text-tx-1 hover:bg-bg-2"
              >
                <Folder className="h-3 w-3" />
                {folderSummaries.find((item) => item.id === folder)?.name ?? t('list.default_folder')}
                <span aria-hidden className="text-tx-3">×</span>
              </button>
            )}
          </div>
        }
        state={listState}
      >
        <DataTable
          className="table-fixed"
          rows={resources}
          rowKey={(resource) => `${resource.kind}:${resource.kind === 'folder' ? resource.folder.id : resource.dashboard.id}`}
          onRowClick={(resource) => {
            if (resource.kind === 'folder') {
              if (normalizedSearch === '') {
                toggleFolderCollapsed(resource.folder.id);
              }
              return;
            }
            nav(`/dashboards/${resource.dashboard.id}`);
          }}
          columns={[
            {
              key: 'dashboard',
              header: t('list.columns.resource'),
              width: '38%',
              cell: (resource) => {
                const isFolder = resource.kind === 'folder';
                const id = isFolder
                  ? resource.folder.id
                  : resource.dashboard.id;
                const name = isFolder
                  ? resource.folder.name
                  : resource.dashboard.name;
                const collapsed =
                  isFolder && effectiveCollapsedFolderIds.has(id);
                return (
                  <DashboardHierarchyResource
                    kind={resource.kind}
                    name={name}
                    depth={resource.depth}
                    expandable={isFolder && expandableFolderIds.has(id)}
                    collapsed={collapsed}
                    collapseDisabled={normalizedSearch !== ''}
                    expandLabel={t('folders.expand_folder', { name })}
                    collapseLabel={t('folders.collapse_folder', { name })}
                    onToggle={() => toggleFolderCollapsed(id)}
                  />
                );
              },
            },
            {
              key: 'type',
              header: t('list.columns.type'),
              cell: (resource) => resource.kind === 'folder' ? t('list.types.folder') : t('list.types.dashboard'),
              width: 120,
            },
            {
              key: 'tags',
              header: t('list.columns.tags'),
              cell: (resource) => resource.kind === 'folder'
                ? <span className="text-tx-3">—</span>
                : resource.dashboard.tags.map((tag) => <Pill key={tag} className="mr-1">#{tag}</Pill>),
            },
            {
              key: 'panels',
              header: t('list.columns.panels'),
              cell: (resource) => resource.kind === 'folder' ? resource.folder.panels : resource.dashboard.panels,
              className: 'text-right',
              headerClassName: 'text-right',
              width: 96,
            },
            {
              key: 'updated',
              header: t('list.columns.updated'),
              cell: (resource) => resource.kind === 'folder' ? '—' : resource.dashboard.edited,
              width: 120,
            },
            {
              key: 'actions',
              header: '',
              cell: (resource) => resource.kind === 'dashboard' ? (
                <div onClick={(event) => event.stopPropagation()} className="flex justify-end">
                  <DashboardActions dashboard={resource.dashboard} {...actionProps} />
                </div>
              ) : null,
              width: 100,
            },
          ]}
        />
      </ListPage>
      <DashboardFolderManager
        open={foldersOpen}
        activeFolder={folder}
        allLabel={t('list.all_folders')}
        folders={folderSummaries}
        apiFolders={apiFolders}
        loading={foldersQuery.isLoading}
        error={foldersQuery.error}
        onOpenChange={setFoldersOpen}
        onSelectFolder={setFolder}
      />
      <ConfirmDialog
        open={deleteTarget !== null}
        title={t('list.delete_title', { title: deleteTarget?.name ?? '' })}
        description={t('list.delete_description')}
        confirmLabel={deleteMutation.isPending ? t('list.deleting') : t('list.delete_confirm')}
        cancelLabel={t('folders.cancel')}
        destructive
        disabled={deleteAccess.disabled}
        disabledReason={deleteAccess.reason}
        onOpenChange={(open) => {
          if (!open && !deleteMutation.isPending) setDeleteTarget(null);
        }}
        onConfirm={() => {
          if (deleteTarget && deleteAccess.allowed) {
            deleteMutation.mutate(deleteTarget);
          }
        }}
      />
      {sharingDashboard && (
        <ResourceShareDialog
          open
          onOpenChange={(open) => {
            if (!open) setSharingDashboard(null);
          }}
          resourceType="dashboard"
          resourceId={sharingDashboard.id}
          title={sharingDashboard.name}
          resourceTags={sharingDashboard.tags}
          variableNames={dashboardDefinitionFromApi(
            sharingDashboard.source,
          ).variables.map((variable) => variable.name)}
        />
      )}
    </>
  );
}

interface DashboardActionProps {
  dashboard: DisplayDashboard;
  folderOptions: FolderSummary[];
  onDelete: (dashboard: DisplayDashboard) => void;
  onDuplicate: (dashboard: DisplayDashboard) => void;
  onEdit: (dashboard: DisplayDashboard) => void;
  onExport: (dashboard: DisplayDashboard) => void;
  onFavorite: (dashboard: DisplayDashboard) => void;
  onMove: (dashboard: DisplayDashboard, folderId: string | undefined) => void;
  onOpen: (dashboard: DisplayDashboard) => void;
  onShare: (dashboard: DisplayDashboard) => void;
  onSetNoc: (dashboard: DisplayDashboard) => void;
  createAccess: ActionAccess;
  editAccess: ActionAccess;
  deleteAccess: ActionAccess;
  shareAccess: ActionAccess;
  sharingId: string | null;
  t: TFunction<'dashboards'>;
}

function DashboardActions({
  dashboard,
  folderOptions,
  onDelete,
  onDuplicate,
  onEdit,
  onExport,
  onFavorite,
  onMove,
  onOpen,
  onShare,
  onSetNoc,
  createAccess,
  editAccess,
  deleteAccess,
  shareAccess,
  sharingId,
  t,
}: DashboardActionProps) {
  return (
    <div className="flex items-center gap-1">
      <button
        type="button"
        aria-label={dashboard.starred ? t('list.actions.unfavorite') : t('list.actions.favorite')}
        title={dashboard.starred ? t('list.actions.unfavorite') : t('list.actions.favorite')}
        onClick={(event) => {
          event.stopPropagation();
          onFavorite(dashboard);
        }}
        className={`grid h-8 w-8 place-items-center rounded-md hover:bg-bg-3 ${
          dashboard.starred ? 'text-yellow' : 'text-tx-3 hover:text-tx-0'
        }`}
      >
        <Star className="h-3.5 w-3.5" fill={dashboard.starred ? 'currentColor' : 'none'} />
      </button>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            aria-label={t('list.actions.more', { title: dashboard.name })}
            onClick={(event) => event.stopPropagation()}
            className="grid h-8 w-8 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0"
          >
            <Ellipsis className="h-4 w-4" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-48">
          <DropdownMenuItem onSelect={() => onOpen(dashboard)}>
            <Eye className="h-3.5 w-3.5" /> {t('list.actions.open')}
          </DropdownMenuItem>
          <DropdownMenuItem
            disabled={sharingId === dashboard.id || shareAccess.disabled}
            disabledReason={
              sharingId !== dashboard.id ? shareAccess.reason : undefined
            }
            onSelect={() => onShare(dashboard)}
          >
            {sharingId === dashboard.id ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Share2 className="h-3.5 w-3.5" />
            )}
            {t('list.actions.share')}
          </DropdownMenuItem>
          <DropdownMenuItem
            disabled={editAccess.disabled}
            disabledReason={editAccess.reason}
            onSelect={() => onEdit(dashboard)}
          >
            <Pencil className="h-3.5 w-3.5" /> {t('list.actions.edit')}
          </DropdownMenuItem>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger
              disabled={editAccess.disabled}
              disabledReason={editAccess.reason}
            >
              <FolderOpen className="h-3.5 w-3.5" /> {t('list.actions.move')}
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="min-w-44">
              {folderOptions.map((folderOption) => (
                <DropdownMenuItem
                  key={folderOption.id}
                  disabled={(dashboard.folderId ?? DEFAULT_FOLDER) === folderOption.id}
                  onSelect={() => onMove(dashboard, folderOption.id === DEFAULT_FOLDER ? undefined : folderOption.id)}
                >
                  {folderOption.name}
                </DropdownMenuItem>
              ))}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuItem
            disabled={createAccess.disabled}
            disabledReason={createAccess.reason}
            onSelect={() => onDuplicate(dashboard)}
          >
            <Copy className="h-3.5 w-3.5" /> {t('list.actions.copy')}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => onExport(dashboard)}>
            <Download className="h-3.5 w-3.5" /> {t('list.actions.export')}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => onSetNoc(dashboard)}>
            <Maximize2 className="h-3.5 w-3.5" /> {t('list.actions.set_noc')}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            className="text-red focus:text-red"
            disabled={deleteAccess.disabled}
            disabledReason={deleteAccess.reason}
            onSelect={() => onDelete(dashboard)}
          >
            <Trash2 className="h-3.5 w-3.5" /> {t('list.actions.delete')}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

export { DashboardView } from '@/dashboard-engine/DashboardView';
