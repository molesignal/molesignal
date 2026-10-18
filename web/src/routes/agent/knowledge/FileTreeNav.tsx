import { ChevronRight, FileText, Folder } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ENTRY_FILE } from '@/api/agent/knowledge';
import { cn } from '@/shell/lib/cn';

import type { TreeNode } from './fileTree';

/** The files of a knowledge base as a tree: folders open and close, a file opens in the editor. */
export function FileTreeNav({
  nodes,
  selected,
  open,
  onToggle,
  onSelect,
}: {
  nodes: readonly TreeNode[];
  selected: string | null;
  open: ReadonlySet<string>;
  onToggle: (path: string) => void;
  onSelect: (path: string) => void;
}) {
  const { t } = useTranslation('agent');
  return (
    <ul role="tree" aria-label={t('knowledge.files_label')} className="flex flex-col gap-px">
      {nodes.map((node) => (
        <TreeItem
          key={node.path}
          node={node}
          depth={0}
          selected={selected}
          open={open}
          onToggle={onToggle}
          onSelect={onSelect}
        />
      ))}
    </ul>
  );
}

function TreeItem({
  node,
  depth,
  selected,
  open,
  onToggle,
  onSelect,
}: {
  node: TreeNode;
  depth: number;
  selected: string | null;
  open: ReadonlySet<string>;
  onToggle: (path: string) => void;
  onSelect: (path: string) => void;
}) {
  const { t } = useTranslation('agent');
  const indent: React.CSSProperties = { paddingLeft: `${8 + depth * 14}px` };

  if (node.kind === 'folder') {
    const expanded = open.has(node.path);
    return (
      <li role="treeitem" aria-expanded={expanded} aria-selected={false}>
        <button
          type="button"
          onClick={() => onToggle(node.path)}
          style={indent}
          className="flex h-8 w-full items-center gap-1.5 rounded-md pr-2 text-left text-xs text-tx-1 hover:bg-bg-2"
        >
          <ChevronRight
            aria-hidden="true"
            className={cn('h-3.5 w-3.5 shrink-0 text-tx-3 transition-transform duration-fast', expanded && 'rotate-90')}
          />
          <Folder aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-tx-3" />
          <span className="truncate font-strong">{node.name}</span>
        </button>
        {expanded && (
          <ul role="group" className="flex flex-col gap-px">
            {node.children.map((child) => (
              <TreeItem
                key={child.path}
                node={child}
                depth={depth + 1}
                selected={selected}
                open={open}
                onToggle={onToggle}
                onSelect={onSelect}
              />
            ))}
          </ul>
        )}
      </li>
    );
  }

  const active = selected === node.path;
  return (
    <li role="treeitem" aria-selected={active}>
      <button
        type="button"
        onClick={() => onSelect(node.path)}
        style={indent}
        data-agent-knowledge-file={node.path}
        className={cn(
          'flex h-8 w-full items-center gap-1.5 rounded-md pr-2 text-left text-xs',
          active ? 'bg-bg-2 text-tx-0' : 'text-tx-1 hover:bg-bg-2',
        )}
      >
        <span className="w-3.5 shrink-0" />
        <FileText aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-tx-3" />
        <span className={cn('truncate', active && 'font-strong')}>{node.name}</span>
        {node.path === ENTRY_FILE && (
          <span className="ml-auto shrink-0 rounded bg-indigo/10 px-1.5 type-micro font-strong text-indigo">
            {t('knowledge.entry_badge')}
          </span>
        )}
      </button>
    </li>
  );
}
