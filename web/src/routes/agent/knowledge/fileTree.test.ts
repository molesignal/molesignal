import { describe, expect, it } from 'vitest';

import type { KnowledgeFileMeta } from '@/api/agent/knowledge';

import { buildTree, isMarkdown, parentFolders } from './fileTree';

const file = (path: string): KnowledgeFileMeta => ({
  path,
  size_bytes: 10,
  updated_by: 'u1',
  updated_at_micros: 1,
});

describe('the files of a knowledge base', () => {
  it('are folders and files, the entry file first, then folders, then files', () => {
    const tree = buildTree([
      file('runbooks/payments.md'),
      file('access.md'),
      file('MOLE.md'),
      file('runbooks/api/5xx.md'),
      file('services.md'),
    ]);

    expect(tree.map((node) => node.path)).toEqual(['MOLE.md', 'runbooks', 'access.md', 'services.md']);

    const runbooks = tree[1];
    expect(runbooks?.kind).toBe('folder');
    if (runbooks?.kind === 'folder') {
      expect(runbooks.children.map((node) => node.path)).toEqual([
        'runbooks/api',
        'runbooks/payments.md',
      ]);
    }
  });

  it('open the folders a file is under and show Markdown as Markdown', () => {
    expect(parentFolders('runbooks/api/5xx.md')).toEqual(['runbooks', 'runbooks/api']);
    expect(parentFolders('MOLE.md')).toEqual([]);
    expect(isMarkdown('MOLE.md')).toBe(true);
    expect(isMarkdown('clusters.yaml')).toBe(false);
  });
});
