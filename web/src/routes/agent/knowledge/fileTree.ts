import { ENTRY_FILE, type KnowledgeFileMeta } from '@/api/agent/knowledge';

export interface TreeFile {
  kind: 'file';
  name: string;
  path: string;
  file: KnowledgeFileMeta;
}

export interface TreeFolder {
  kind: 'folder';
  name: string;
  path: string;
  children: TreeNode[];
}

export type TreeNode = TreeFile | TreeFolder;

function sortNodes(nodes: TreeNode[]): TreeNode[] {
  // The entry file first, then folders, then files, each by name.
  const rank = (node: TreeNode) =>
    node.kind === 'file' && node.path === ENTRY_FILE ? 0 : node.kind === 'folder' ? 1 : 2;
  return nodes
    .map((node) => (node.kind === 'folder' ? { ...node, children: sortNodes(node.children) } : node))
    .sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
}

/** The files of a knowledge base as folders and files, the entry file on top. */
export function buildTree(files: readonly KnowledgeFileMeta[]): TreeNode[] {
  const root: TreeFolder = { kind: 'folder', name: '', path: '', children: [] };
  for (const file of files) {
    const parts = file.path.split('/');
    let folder = root;
    parts.slice(0, -1).forEach((part, index) => {
      const path = parts.slice(0, index + 1).join('/');
      let next = folder.children.find(
        (node): node is TreeFolder => node.kind === 'folder' && node.name === part,
      );
      if (!next) {
        next = { kind: 'folder', name: part, path, children: [] };
        folder.children.push(next);
      }
      folder = next;
    });
    folder.children.push({ kind: 'file', name: parts[parts.length - 1] ?? file.path, path: file.path, file });
  }
  return sortNodes(root.children);
}

/** The folders a path is under, outermost first: opened so the file shows. */
export function parentFolders(path: string): string[] {
  const parts = path.split('/').slice(0, -1);
  return parts.map((_, index) => parts.slice(0, index + 1).join('/'));
}

/** Whether a file is shown as Markdown. */
export function isMarkdown(path: string): boolean {
  return /\.(md|markdown|mdx)$/i.test(path);
}

/** A file picked for upload: its path in the folder when a folder was picked, else its name. */
export function uploadPath(file: File): string {
  const relative = (file as File & { webkitRelativePath?: string }).webkitRelativePath;
  return relative && relative.length > 0 ? relative : file.name;
}
