import { describe, expect, it } from 'vitest';

import { buildDashboardHierarchy } from './hierarchy';

interface Folder {
  id: string;
  name: string;
  parentId: string | undefined;
}

interface Dashboard {
  id: string;
  name: string;
  folderId: string;
}

const folders: Folder[] = [
  { id: 'default', name: 'Default', parentId: undefined },
  { id: 'platform', name: 'Platform', parentId: undefined },
  { id: 'api', name: 'API', parentId: 'platform' },
  { id: 'database', name: 'Database', parentId: 'platform' },
];

const dashboards: Dashboard[] = [
  { id: 'overview', name: 'Overview', folderId: 'default' },
  { id: 'latency', name: 'Latency', folderId: 'api' },
  { id: 'errors', name: 'Errors', folderId: 'api' },
  { id: 'postgres', name: 'Postgres', folderId: 'database' },
];

describe('buildDashboardHierarchy', () => {
  it('flattens folders and dashboards in hierarchy order', () => {
    const rows = buildDashboardHierarchy({
      folders,
      dashboards,
      defaultFolderId: 'default',
      includeEmptyFolders: true,
    });

    expect(
      rows.map((row) => [
        row.kind,
        row.kind === 'folder' ? row.folder.id : row.dashboard.id,
        row.depth,
      ]),
    ).toEqual([
      ['folder', 'default', 0],
      ['dashboard', 'overview', 1],
      ['folder', 'platform', 0],
      ['folder', 'api', 1],
      ['dashboard', 'errors', 2],
      ['dashboard', 'latency', 2],
      ['folder', 'database', 1],
      ['dashboard', 'postgres', 2],
    ]);
  });

  it('shows a selected folder subtree with relative indentation', () => {
    const rows = buildDashboardHierarchy({
      folders,
      dashboards,
      defaultFolderId: 'default',
      rootFolderId: 'platform',
      includeEmptyFolders: true,
    });

    expect(
      rows.map((row) => [
        row.kind === 'folder' ? row.folder.id : row.dashboard.id,
        row.depth,
      ]),
    ).toEqual([
      ['platform', 0],
      ['api', 1],
      ['errors', 2],
      ['latency', 2],
      ['database', 1],
      ['postgres', 2],
    ]);
  });

  it('keeps ancestors for filtered dashboard results', () => {
    const rows = buildDashboardHierarchy({
      folders,
      dashboards: dashboards.filter((dashboard) => dashboard.id === 'latency'),
      defaultFolderId: 'default',
    });

    expect(
      rows.map((row) => [
        row.kind === 'folder' ? row.folder.id : row.dashboard.id,
        row.depth,
      ]),
    ).toEqual([
      ['platform', 0],
      ['api', 1],
      ['latency', 2],
    ]);
  });

  it('hides every descendant of a collapsed folder', () => {
    const rows = buildDashboardHierarchy({
      folders,
      dashboards,
      defaultFolderId: 'default',
      includeEmptyFolders: true,
      collapsedFolderIds: new Set(['platform']),
    });

    expect(
      rows.map((row) =>
        row.kind === 'folder' ? row.folder.id : row.dashboard.id,
      ),
    ).toEqual(['default', 'overview', 'platform']);
  });
});
