export interface HierarchyFolder {
  id: string;
  name: string;
  parentId: string | undefined;
}

export interface HierarchyDashboard {
  id: string;
  name: string;
  folderId: string | undefined;
}

export type DashboardHierarchyRow<
  TFolder extends HierarchyFolder,
  TDashboard extends HierarchyDashboard,
> =
  | { kind: 'folder'; folder: TFolder; depth: number }
  | { kind: 'dashboard'; dashboard: TDashboard; depth: number };

interface BuildDashboardHierarchyOptions<
  TFolder extends HierarchyFolder,
  TDashboard extends HierarchyDashboard,
> {
  folders: readonly TFolder[];
  dashboards: readonly TDashboard[];
  defaultFolderId: string;
  rootFolderId?: string | undefined;
  includeEmptyFolders?: boolean;
  additionalVisibleFolderIds?: ReadonlySet<string>;
  collapsedFolderIds?: ReadonlySet<string>;
}

function compareNamed(
  left: { id: string; name: string },
  right: { id: string; name: string },
  defaultFolderId: string,
): number {
  if (left.id === defaultFolderId) return -1;
  if (right.id === defaultFolderId) return 1;
  return left.name.localeCompare(right.name);
}

/**
 * Produces the single list presentation used by the dashboards page.
 * Folders are emitted before their descendants and dashboards receive one
 * additional indentation level beneath their containing folder.
 */
export function buildDashboardHierarchy<
  TFolder extends HierarchyFolder,
  TDashboard extends HierarchyDashboard,
>({
  folders,
  dashboards,
  defaultFolderId,
  rootFolderId,
  includeEmptyFolders = false,
  additionalVisibleFolderIds = new Set(),
  collapsedFolderIds = new Set(),
}: BuildDashboardHierarchyOptions<
  TFolder,
  TDashboard
>): DashboardHierarchyRow<TFolder, TDashboard>[] {
  const folderById = new Map(folders.map((folder) => [folder.id, folder]));
  const childIdsByParent = new Map<string, string[]>();

  for (const folder of folders) {
    if (!folder.parentId || !folderById.has(folder.parentId)) continue;
    const childIds = childIdsByParent.get(folder.parentId) ?? [];
    childIds.push(folder.id);
    childIdsByParent.set(folder.parentId, childIds);
  }

  const scopedFolderIds = new Set<string>();
  const collectScope = (folderId: string) => {
    if (scopedFolderIds.has(folderId) || !folderById.has(folderId)) return;
    scopedFolderIds.add(folderId);
    for (const childId of childIdsByParent.get(folderId) ?? []) {
      collectScope(childId);
    }
  };

  if (rootFolderId) {
    collectScope(rootFolderId);
  } else {
    for (const folder of folders) scopedFolderIds.add(folder.id);
  }

  const scopedDashboards = dashboards.filter((dashboard) =>
    scopedFolderIds.has(dashboard.folderId ?? defaultFolderId),
  );
  const visibleFolderIds = includeEmptyFolders
    ? new Set(scopedFolderIds)
    : new Set<string>();

  const includeFolderAndAncestors = (folderId: string) => {
    let currentId: string | undefined = folderId;
    const visited = new Set<string>();
    while (
      currentId &&
      scopedFolderIds.has(currentId) &&
      !visited.has(currentId)
    ) {
      visited.add(currentId);
      visibleFolderIds.add(currentId);
      currentId = folderById.get(currentId)?.parentId;
    }
  };

  for (const dashboard of scopedDashboards) {
    includeFolderAndAncestors(dashboard.folderId ?? defaultFolderId);
  }
  for (const folderId of additionalVisibleFolderIds) {
    includeFolderAndAncestors(folderId);
  }

  const visibleChildrenByParent = new Map<string, TFolder[]>();
  const rootFolders: TFolder[] = [];
  for (const folder of folders) {
    if (!visibleFolderIds.has(folder.id)) continue;
    if (folder.parentId && visibleFolderIds.has(folder.parentId)) {
      const children = visibleChildrenByParent.get(folder.parentId) ?? [];
      children.push(folder);
      visibleChildrenByParent.set(folder.parentId, children);
    } else {
      rootFolders.push(folder);
    }
  }

  const dashboardsByFolder = new Map<string, TDashboard[]>();
  for (const dashboard of scopedDashboards) {
    const folderId = dashboard.folderId ?? defaultFolderId;
    if (!visibleFolderIds.has(folderId)) continue;
    const folderDashboards = dashboardsByFolder.get(folderId) ?? [];
    folderDashboards.push(dashboard);
    dashboardsByFolder.set(folderId, folderDashboards);
  }

  const sortFolders = (items: TFolder[]) =>
    items.sort((left, right) =>
      compareNamed(left, right, defaultFolderId),
    );
  const sortDashboards = (items: TDashboard[]) =>
    items.sort((left, right) => left.name.localeCompare(right.name));

  sortFolders(rootFolders);
  for (const children of visibleChildrenByParent.values()) {
    sortFolders(children);
  }
  for (const folderDashboards of dashboardsByFolder.values()) {
    sortDashboards(folderDashboards);
  }

  const rows: DashboardHierarchyRow<TFolder, TDashboard>[] = [];
  const visitedFolderIds = new Set<string>();
  const hiddenFolderIds = new Set<string>();
  const hideDescendants = (folderId: string) => {
    for (const child of visibleChildrenByParent.get(folderId) ?? []) {
      if (hiddenFolderIds.has(child.id)) continue;
      hiddenFolderIds.add(child.id);
      hideDescendants(child.id);
    }
  };
  const appendFolder = (folder: TFolder, depth: number) => {
    if (
      visitedFolderIds.has(folder.id) ||
      hiddenFolderIds.has(folder.id)
    ) {
      return;
    }
    visitedFolderIds.add(folder.id);
    rows.push({ kind: 'folder', folder, depth });

    if (collapsedFolderIds.has(folder.id)) {
      hideDescendants(folder.id);
      return;
    }

    for (const child of visibleChildrenByParent.get(folder.id) ?? []) {
      appendFolder(child, depth + 1);
    }
    for (const dashboard of dashboardsByFolder.get(folder.id) ?? []) {
      rows.push({ kind: 'dashboard', dashboard, depth: depth + 1 });
    }
  };

  for (const folder of rootFolders) appendFolder(folder, 0);

  // Cyclic parent data has no natural root. Keep those folders visible and
  // deterministic instead of dropping their dashboards from the list.
  for (const folder of sortFolders(
    folders.filter(
      (candidate) =>
        visibleFolderIds.has(candidate.id) &&
        !visitedFolderIds.has(candidate.id) &&
        !hiddenFolderIds.has(candidate.id),
    ),
  )) {
    appendFolder(folder, 0);
  }

  return rows;
}
