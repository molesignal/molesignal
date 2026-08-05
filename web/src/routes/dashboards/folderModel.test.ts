import { describe, expect, it } from 'vitest';

import type { Folder } from '@/api/folders';

import {
  ROOT_PARENT,
  type FolderSummary,
  folderCanContain,
  folderPath,
  folderPayload,
  folderSubtreeHeight,
  isDescendant,
  visibleFolderTree,
} from './folderModel';

const root: FolderSummary = {
  id: 'platform',
  name: 'Platform',
  parentId: undefined,
  dashboards: 1,
  panels: 2,
  managed: true,
  depth: 0,
};
const child: FolderSummary = {
  id: 'api',
  name: 'API',
  parentId: 'platform',
  dashboards: 2,
  panels: 4,
  managed: true,
  depth: 1,
};
const grandchild: FolderSummary = {
  id: 'payments',
  name: 'Payments',
  parentId: 'api',
  dashboards: 1,
  panels: 3,
  managed: true,
  depth: 2,
};

describe('dashboard folder model', () => {
  it('builds a readable nested path', () => {
    const folders = new Map([
      [root.id, root],
      [child.id, child],
    ]);

    expect(folderPath(child, folders)).toBe('Platform / API');
  });

  it('normalizes create and update payloads', () => {
    expect(folderPayload('  Production  ', ROOT_PARENT)).toEqual({
      name: 'Production',
    });
    expect(folderPayload(' API ', root.id)).toEqual({
      name: 'API',
      parent_id: 'platform',
    });
  });

  it('prevents moving a folder below its own descendant', () => {
    const folders: Folder[] = [
      { id: root.id, org_id: 'org', name: root.name },
      {
        id: child.id,
        org_id: 'org',
        name: child.name,
        parent_id: root.id,
      },
    ];

    expect(isDescendant(folders, root.id, child.id)).toBe(true);
    expect(isDescendant(folders, child.id, root.id)).toBe(false);
  });

  it('only shows descendants of expanded folders', () => {
    const folders = [root, child, grandchild];

    expect(
      visibleFolderTree(folders, new Set()).map((folder) => folder.id),
    ).toEqual(['platform']);
    expect(
      visibleFolderTree(folders, new Set(['platform'])).map(
        (folder) => folder.id,
      ),
    ).toEqual(['platform', 'api']);
    expect(
      visibleFolderTree(folders, new Set(['platform', 'api'])).map(
        (folder) => folder.id,
      ),
    ).toEqual(['platform', 'api', 'payments']);
    expect(
      visibleFolderTree(folders, new Set(), {
        rootExpanded: false,
      }),
    ).toEqual([]);
  });

  it('reveals the complete path for search results', () => {
    expect(
      visibleFolderTree([root, child, grandchild], new Set(), {
        rootExpanded: false,
        search: 'payments',
      }).map((folder) => folder.id),
    ).toEqual(['platform', 'api', 'payments']);
  });

  it('allows at most three directory levels', () => {
    const folders = [root, child, grandchild];

    expect(folderCanContain(root)).toBe(true);
    expect(folderCanContain(child)).toBe(true);
    expect(folderCanContain(grandchild)).toBe(false);
    expect(folderSubtreeHeight(folders, root.id)).toBe(3);
    expect(folderSubtreeHeight(folders, child.id)).toBe(2);
    expect(folderCanContain(root, folderSubtreeHeight(folders, child.id)))
      .toBe(true);
    expect(folderCanContain(child, folderSubtreeHeight(folders, child.id)))
      .toBe(false);
  });
});
