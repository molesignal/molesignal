import type * as foldersApi from '@/api/folders';

export interface FolderSummary {
  id: string;
  name: string;
  parentId: string | undefined;
  dashboards: number;
  panels: number;
  managed: boolean;
  depth: number;
}

export const ALL_FOLDERS = '__all__';
export const DEFAULT_FOLDER = '__default__';
export const ROOT_PARENT = '__root__';
export const MAX_FOLDER_LEVELS = 3;

export function childFolderCounts(
  folders: readonly foldersApi.Folder[],
): Map<string, number> {
  const counts = new Map<string, number>();
  for (const folder of folders) {
    if (!folder.parent_id) continue;
    counts.set(
      folder.parent_id,
      (counts.get(folder.parent_id) ?? 0) + 1,
    );
  }
  return counts;
}

export function folderAncestorIds(
  folderId: string,
  byId: ReadonlyMap<string, FolderSummary>,
): string[] {
  const ids: string[] = [];
  let parentId = byId.get(folderId)?.parentId;
  const visited = new Set<string>();
  while (parentId && !visited.has(parentId)) {
    visited.add(parentId);
    ids.push(parentId);
    parentId = byId.get(parentId)?.parentId;
  }
  return ids;
}

export function folderCanContain(
  parent: FolderSummary | undefined,
  subtreeHeight = 1,
): boolean {
  const parentLevel = parent ? parent.depth + 1 : 0;
  return parentLevel + subtreeHeight <= MAX_FOLDER_LEVELS;
}

export function folderSubtreeHeight(
  folders: readonly FolderSummary[],
  folderId: string,
): number {
  const byId = new Map(folders.map((folder) => [folder.id, folder]));
  let height = 1;
  for (const candidate of folders) {
    let current = candidate;
    let distance = 0;
    const visited = new Set<string>();
    while (current.parentId && !visited.has(current.id)) {
      visited.add(current.id);
      distance += 1;
      if (current.parentId === folderId) {
        height = Math.max(height, distance + 1);
        break;
      }
      const parent = byId.get(current.parentId);
      if (!parent) break;
      current = parent;
    }
  }
  return height;
}

export function folderParentValue(parentId: string | undefined): string {
  return parentId ?? ROOT_PARENT;
}

export function folderPayload(
  name: string,
  parentValue: string,
): foldersApi.FolderInput {
  const trimmedName = name.trim();
  return parentValue === ROOT_PARENT
    ? { name: trimmedName }
    : { name: trimmedName, parent_id: parentValue };
}

export function folderPath(
  folder: FolderSummary,
  byId: ReadonlyMap<string, FolderSummary>,
): string {
  const names: string[] = [folder.name];
  let parentId = folder.parentId;
  let guard = 0;
  while (parentId && guard < byId.size + 1) {
    const parent = byId.get(parentId);
    if (!parent) break;
    names.unshift(parent.name);
    parentId = parent.parentId;
    guard += 1;
  }
  return names.join(' / ');
}

export function visibleFolderTree(
  folders: readonly FolderSummary[],
  expandedFolderIds: ReadonlySet<string>,
  {
    rootExpanded = true,
    search = '',
  }: {
    rootExpanded?: boolean;
    search?: string;
  } = {},
): FolderSummary[] {
  const byId = new Map(folders.map((folder) => [folder.id, folder]));
  const normalizedSearch = search.trim().toLowerCase();

  if (normalizedSearch) {
    const visibleIds = new Set<string>();
    for (const folder of folders) {
      if (!folderPath(folder, byId).toLowerCase().includes(normalizedSearch)) {
        continue;
      }
      let current: FolderSummary | undefined = folder;
      const visited = new Set<string>();
      while (current && !visited.has(current.id)) {
        visited.add(current.id);
        visibleIds.add(current.id);
        current = current.parentId
          ? byId.get(current.parentId)
          : undefined;
      }
    }
    return folders.filter((folder) => visibleIds.has(folder.id));
  }

  if (!rootExpanded) return [];

  return folders.filter((folder) => {
    let parentId = folder.parentId;
    const visited = new Set<string>();
    while (parentId && byId.has(parentId) && !visited.has(parentId)) {
      if (!expandedFolderIds.has(parentId)) return false;
      visited.add(parentId);
      parentId = byId.get(parentId)?.parentId;
    }
    return true;
  });
}

export function isDescendant(
  folders: readonly foldersApi.Folder[],
  folderId: string,
  candidateId: string,
): boolean {
  const parentById = new Map(
    folders.map((folder) => [folder.id, folder.parent_id]),
  );
  let parentId = parentById.get(candidateId);
  let guard = 0;
  while (parentId && guard < folders.length + 1) {
    if (parentId === folderId) return true;
    parentId = parentById.get(parentId);
    guard += 1;
  }
  return false;
}
