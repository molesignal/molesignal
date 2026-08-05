import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  LayoutDashboard,
} from 'lucide-react';

interface DashboardHierarchyResourceProps {
  kind: 'folder' | 'dashboard';
  name: string;
  depth: number;
  expandable?: boolean;
  collapsed?: boolean;
  collapseDisabled?: boolean;
  expandLabel?: string;
  collapseLabel?: string;
  onToggle?: () => void;
}

export function DashboardHierarchyResource({
  kind,
  name,
  depth,
  expandable = false,
  collapsed = false,
  collapseDisabled = false,
  expandLabel,
  collapseLabel,
  onToggle,
}: DashboardHierarchyResourceProps) {
  const FolderIcon = expandable && !collapsed ? FolderOpen : Folder;

  return (
    <div
      className="flex min-w-0 items-center gap-2"
      data-hierarchy-depth={depth}
      style={{ paddingInlineStart: `${depth * 20}px` }}
    >
      {kind === 'folder' && expandable ? (
        <button
          type="button"
          aria-expanded={!collapsed}
          aria-label={collapsed ? expandLabel : collapseLabel}
          disabled={collapseDisabled}
          onClick={(event) => {
            event.stopPropagation();
            onToggle?.();
          }}
          className="grid h-7 w-7 shrink-0 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0 disabled:cursor-default disabled:hover:bg-transparent disabled:hover:text-tx-3"
        >
          {collapsed ? (
            <ChevronRight className="h-3.5 w-3.5" />
          ) : (
            <ChevronDown className="h-3.5 w-3.5" />
          )}
        </button>
      ) : (
        <span aria-hidden="true" className="h-7 w-7 shrink-0" />
      )}

      {kind === 'folder' ? (
        <FolderIcon
          aria-hidden="true"
          data-testid="folder-resource-icon"
          className="h-4 w-4 shrink-0 text-tx-3"
        />
      ) : (
        <LayoutDashboard
          aria-hidden="true"
          data-testid="dashboard-resource-icon"
          className="h-4 w-4 shrink-0 text-tx-3"
        />
      )}

      <span className="truncate font-strong text-tx-0">{name}</span>
    </div>
  );
}
