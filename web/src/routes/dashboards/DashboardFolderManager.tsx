import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Plus, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as foldersApi from '@/api/folders';
import { toApiError } from '@/lib/http';
import {
  restrictActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';

import {
  FolderCreateForm,
  FolderDetails,
  FolderTreeRow,
} from './DashboardFolderManagerPanels';
import {
  ALL_FOLDERS,
  ROOT_PARENT,
  type FolderSummary,
  childFolderCounts,
  folderAncestorIds,
  folderCanContain,
  folderParentValue,
  folderPath,
  folderPayload,
  folderSubtreeHeight,
  isDescendant,
  visibleFolderTree,
} from './folderModel';

type ManagerMode = 'browse' | 'create';

export interface DashboardFolderManagerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  folders: FolderSummary[];
  apiFolders: foldersApi.Folder[];
  loading: boolean;
  error: unknown;
  activeFolder: string;
  allLabel: string;
  onSelectFolder: (folder: string) => void;
}

export function DashboardFolderManager({
  open,
  onOpenChange,
  folders,
  apiFolders,
  loading,
  error,
  activeFolder,
  allLabel,
  onSelectFolder,
}: DashboardFolderManagerProps) {
  const { t } = useTranslation('dashboards');
  const qc = useQueryClient();
  const createAccess = useActionAccess({ permission: 'dashboards.create' });
  const editAccess = useActionAccess({ permission: 'dashboards.edit' });
  const deleteAccess = useActionAccess({ permission: 'dashboards.delete' });
  const [mode, setMode] = React.useState<ManagerMode>('browse');
  const [selectedId, setSelectedId] = React.useState(activeFolder);
  const [search, setSearch] = React.useState('');
  const [rootExpanded, setRootExpanded] = React.useState(true);
  const [expandedFolderIds, setExpandedFolderIds] =
    React.useState<Set<string>>(() => new Set());
  const [newName, setNewName] = React.useState('');
  const [newParent, setNewParent] = React.useState(ROOT_PARENT);
  const [editName, setEditName] = React.useState('');
  const [editParent, setEditParent] = React.useState(ROOT_PARENT);
  const [deleteTarget, setDeleteTarget] =
    React.useState<FolderSummary | null>(null);

  const byId = React.useMemo(
    () => new Map(folders.map((folder) => [folder.id, folder])),
    [folders],
  );
  const selected = byId.get(selectedId);
  const managedFolders = React.useMemo(
    () => folders.filter((folder) => folder.managed),
    [folders],
  );
  const childrenByParent = React.useMemo(
    () => childFolderCounts(apiFolders),
    [apiFolders],
  );
  const expandableFolderIds = React.useMemo(
    () => new Set(childrenByParent.keys()),
    [childrenByParent],
  );
  const activeAncestorIds = React.useMemo(
    () => folderAncestorIds(activeFolder, byId),
    [activeFolder, byId],
  );

  React.useEffect(() => {
    if (!open) return;
    setSelectedId(activeFolder);
    setMode('browse');
    setSearch('');
  }, [activeFolder, open]);

  React.useEffect(() => {
    if (!open) return;
    setRootExpanded(true);
    setExpandedFolderIds((current) => {
      const next = new Set(current);
      for (const folderId of activeAncestorIds) next.add(folderId);
      return next;
    });
  }, [activeAncestorIds, open]);

  React.useEffect(() => {
    if (!selected) return;
    setEditName(selected.name);
    setEditParent(folderParentValue(selected.parentId));
  }, [selected]);

  const createParentOptions = React.useMemo(
    () => [
      { value: ROOT_PARENT, label: t('folders.parent_root') },
      ...managedFolders
        .filter((folder) => folderCanContain(folder))
        .map((folder) => ({
          value: folder.id,
          label: folderPath(folder, byId),
        })),
    ],
    [byId, managedFolders, t],
  );
  const editParentOptions = React.useMemo(() => {
    if (!selected?.managed) return createParentOptions;
    const subtreeHeight = folderSubtreeHeight(folders, selected.id);
    return [
      { value: ROOT_PARENT, label: t('folders.parent_root') },
      ...managedFolders
        .filter(
          (folder) =>
            folder.id !== selected.id &&
            !isDescendant(apiFolders, selected.id, folder.id) &&
            folderCanContain(folder, subtreeHeight),
        )
        .map((folder) => ({
          value: folder.id,
          label: folderPath(folder, byId),
        })),
    ];
  }, [
    apiFolders,
    byId,
    createParentOptions,
    folders,
    managedFolders,
    selected,
    t,
  ]);

  const normalizedSearch = search.trim().toLowerCase();
  const visibleFolders = React.useMemo(
    () =>
      visibleFolderTree(folders, expandedFolderIds, {
        rootExpanded,
        search: normalizedSearch,
      }),
    [expandedFolderIds, folders, normalizedSearch, rootExpanded],
  );

  const invalidateFolders = React.useCallback(async () => {
    await Promise.all([
      qc.invalidateQueries({ queryKey: ['folders', 'list'] }),
      qc.invalidateQueries({ queryKey: ['dashboards', 'list'] }),
    ]);
  }, [qc]);

  const createMutation = useMutation({
    mutationFn: foldersApi.create,
    onSuccess: async (folder) => {
      setNewName('');
      setNewParent(ROOT_PARENT);
      setSelectedId(folder.id);
      setMode('browse');
      setRootExpanded(true);
      const parentId = folder.parent_id;
      if (parentId) {
        setExpandedFolderIds((current) => {
          const next = new Set(current);
          next.add(parentId);
          return next;
        });
      }
      await invalidateFolders();
      toast.success(t('folders.toast_created', { name: folder.name }));
    },
    onError: (cause) => {
      toast.error(t('folders.toast_create_failed'), {
        description: toApiError(cause).message,
      });
    },
  });
  const updateMutation = useMutation({
    mutationFn: ({
      id,
      input,
    }: {
      id: string;
      input: foldersApi.FolderInput;
    }) => foldersApi.update(id, input),
    onSuccess: async (folder) => {
      setRootExpanded(true);
      const parentId = folder.parent_id;
      if (parentId) {
        setExpandedFolderIds((current) => {
          const next = new Set(current);
          next.add(parentId);
          return next;
        });
      }
      await invalidateFolders();
      toast.success(t('folders.toast_updated', { name: folder.name }));
    },
    onError: (cause) => {
      toast.error(t('folders.toast_update_failed'), {
        description: toApiError(cause).message,
      });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: foldersApi.remove,
    onSuccess: async () => {
      const deleted = deleteTarget;
      setDeleteTarget(null);
      setSelectedId(ALL_FOLDERS);
      if (deleted?.id === activeFolder) onSelectFolder(ALL_FOLDERS);
      await invalidateFolders();
      toast.success(t('folders.toast_deleted', { name: deleted?.name ?? '' }));
    },
    onError: (cause) => {
      toast.error(t('folders.toast_delete_failed'), {
        description: toApiError(cause).message,
      });
    },
  });

  const beginCreate = (parentId?: string) => {
    const parent = parentId ? byId.get(parentId) : undefined;
    setNewName('');
    setNewParent(
      parentId && parent?.managed && folderCanContain(parent)
        ? parentId
        : ROOT_PARENT,
    );
    setMode('create');
  };
  const toggleFolder = (folderId: string) => {
    setExpandedFolderIds((current) => {
      const next = new Set(current);
      if (next.has(folderId)) next.delete(folderId);
      else next.add(folderId);
      return next;
    });
  };
  const openSelectedFolder = () => {
    onSelectFolder(selected?.id ?? ALL_FOLDERS);
    onOpenChange(false);
  };
  const selectedChildren = selected
    ? (childrenByParent.get(selected.id) ?? 0)
    : apiFolders.filter((folder) => !folder.parent_id).length;
  const selectedDashboards =
    selected?.dashboards ??
    folders.reduce((sum, folder) => sum + folder.dashboards, 0);
  const selectedPanels =
    selected?.panels ??
    folders.reduce((sum, folder) => sum + folder.panels, 0);
  const deleteFolderAccess = selected
    ? restrictActionAccess(
        deleteAccess,
        selected.dashboards === 0 && selectedChildren === 0,
        t('folders.delete_blocked'),
      )
    : deleteAccess;

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="max-w-[960px] overflow-hidden p-0">
          <DialogHeader className="border-b border-bd-0 px-5 py-4">
            <DialogTitle>{t('folders.title')}</DialogTitle>
            <p className="font-sans text-xs text-tx-2">
              {t('folders.subtitle')}
            </p>
          </DialogHeader>

          <div className="grid min-h-[540px] grid-cols-[300px_minmax(0,1fr)]">
            <aside className="min-h-0 border-r border-bd-0 bg-bg-1">
              <div className="flex items-center gap-2 border-b border-bd-0 p-3">
                <div className="flex h-9 min-w-0 flex-1 items-center gap-2 rounded-md border border-bd-1 bg-bg-0 px-3">
                  <Search className="h-3.5 w-3.5 shrink-0 text-tx-3" />
                  <input
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                    placeholder={t('folders.search_placeholder')}
                    className="min-w-0 flex-1 bg-transparent font-sans text-xs text-tx-0 placeholder:text-tx-3 focus:outline-none"
                  />
                </div>
                <ChromeButton
                  size="sm"
                  disabled={createAccess.disabled}
                  disabledReason={createAccess.reason}
                  onClick={() => beginCreate(selected?.id)}
                  aria-label={t('folders.new_folder')}
                  title={t('folders.new_folder')}
                >
                  <Plus className="h-3.5 w-3.5" />
                </ChromeButton>
              </div>
              <nav
                aria-label={t('folders.navigation_label')}
                className="max-h-[474px] overflow-y-auto p-2"
              >
                <FolderTreeRow
                  active={selectedId === ALL_FOLDERS && mode === 'browse'}
                  count={folders.reduce(
                    (sum, folder) => sum + folder.dashboards,
                    0,
                  )}
                  depth={0}
                  expanded={normalizedSearch !== '' || rootExpanded}
                  expandable={normalizedSearch === '' && folders.length > 0}
                  label={allLabel}
                  root
                  onToggle={() => setRootExpanded((current) => !current)}
                  onClick={() => {
                    setSelectedId(ALL_FOLDERS);
                    setMode('browse');
                    if (normalizedSearch === '' && folders.length > 0) {
                      setRootExpanded((current) => !current);
                    }
                  }}
                />
                {loading && (
                  <p className="px-3 py-3 font-sans text-xs text-tx-2">
                    {t('folders.loading')}
                  </p>
                )}
                {error !== null && error !== undefined && (
                  <p className="px-3 py-3 font-sans text-xs text-red">
                    {toApiError(error).message}
                  </p>
                )}
                {!loading &&
                  !error &&
                  visibleFolders.map((folder) => (
                    <FolderTreeRow
                      key={folder.id}
                      active={
                        selectedId === folder.id && mode === 'browse'
                      }
                      count={folder.dashboards}
                      depth={folder.depth + 1}
                      expanded={
                        normalizedSearch !== '' ||
                        expandedFolderIds.has(folder.id)
                      }
                      expandable={
                        normalizedSearch === '' &&
                        expandableFolderIds.has(folder.id)
                      }
                      label={folder.name}
                      managed={folder.managed}
                      onToggle={() => toggleFolder(folder.id)}
                      onClick={() => {
                        setSelectedId(folder.id);
                        setMode('browse');
                        if (
                          normalizedSearch === '' &&
                          expandableFolderIds.has(folder.id)
                        ) {
                          toggleFolder(folder.id);
                        }
                      }}
                    />
                  ))}
                {!loading &&
                  !error &&
                  normalizedSearch &&
                  visibleFolders.length === 0 && (
                    <p className="px-3 py-8 text-center font-sans text-xs text-tx-3">
                      {t('folders.empty_search')}
                    </p>
                  )}
              </nav>
            </aside>

            <section className="min-w-0 bg-bg-0 p-5">
              {mode === 'create' ? (
                <FolderCreateForm
                  name={newName}
                  parent={newParent}
                  parentOptions={createParentOptions}
                  pending={createMutation.isPending}
                  disabled={createAccess.disabled}
                  disabledReason={createAccess.reason}
                  onNameChange={setNewName}
                  onParentChange={setNewParent}
                  onCancel={() => setMode('browse')}
                  onSubmit={() => {
                    if (newName.trim() && createAccess.allowed) {
                      createMutation.mutate(folderPayload(newName, newParent));
                    }
                  }}
                />
              ) : (
                <FolderDetails
                  folder={selected}
                  allLabel={allLabel}
                  childCount={selectedChildren}
                  dashboardCount={selectedDashboards}
                  panelCount={selectedPanels}
                  editName={editName}
                  editParent={editParent}
                  parentOptions={editParentOptions}
                  saving={updateMutation.isPending}
                  editDisabled={editAccess.disabled}
                  editDisabledReason={editAccess.reason}
                  createChildDisabled={
                    selected !== undefined && !folderCanContain(selected)
                  }
                  createChildDisabledReason={t('folders.max_depth')}
                  deleteDisabled={deleteFolderAccess.disabled}
                  deleteDisabledReason={deleteFolderAccess.reason}
                  path={selected ? folderPath(selected, byId) : allLabel}
                  onEditNameChange={setEditName}
                  onEditParentChange={setEditParent}
                  onOpen={openSelectedFolder}
                  onCreateChild={() => beginCreate(selected?.id)}
                  onSave={() => {
                    if (
                      selected?.managed &&
                      editName.trim() &&
                      editAccess.allowed
                    ) {
                      updateMutation.mutate({
                        id: selected.id,
                        input: folderPayload(editName, editParent),
                      });
                    }
                  }}
                  onDelete={() => {
                    if (selected) setDeleteTarget(selected);
                  }}
                />
              )}
            </section>
          </div>
          <DialogFooter className="border-t border-bd-0 px-5 py-3">
            <ChromeButton onClick={() => onOpenChange(false)}>
              {t('folders.close')}
            </ChromeButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        open={deleteTarget !== null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setDeleteTarget(null);
        }}
        title={t('folders.delete_title', {
          name: deleteTarget?.name ?? '',
        })}
        description={t('folders.delete_description')}
        confirmLabel={t('folders.delete_confirm')}
        cancelLabel={t('folders.cancel')}
        destructive
        busy={deleteMutation.isPending}
        disabled={deleteAccess.disabled}
        disabledReason={deleteAccess.reason}
        onConfirm={() => {
          if (deleteTarget && deleteAccess.allowed) {
            deleteMutation.mutate(deleteTarget.id);
          }
        }}
      />
    </>
  );
}
