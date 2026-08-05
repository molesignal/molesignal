import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import type { DashboardElement } from '../schema';

export type DashboardGridInteractionMode = 'move' | 'resize';

export interface DashboardGridEditingConfig {
  selectedIds: ReadonlySet<string>;
  onSelectElement: (elementId: string, additive: boolean) => void;
  onInteractionStart: (
    event: React.PointerEvent<HTMLElement>,
    elementId: string,
    mode: DashboardGridInteractionMode,
  ) => void;
}

interface EditableDashboardGridItemProps {
  element: DashboardElement;
  elementId: string;
  editing: DashboardGridEditingConfig;
  style: React.CSSProperties;
  children: React.ReactNode;
}

export function EditableDashboardGridItem({
  element,
  elementId,
  editing,
  style,
  children,
}: EditableDashboardGridItemProps) {
  const selected = editing.selectedIds.has(elementId);

  return (
    <div
      data-dashboard-edit-element={elementId}
      data-selected={selected ? 'true' : 'false'}
      tabIndex={0}
      onPointerDownCapture={(event) => {
        if (element.kind !== 'panel' || event.button > 0) return;
        const target = event.target as Element;
        if (typeof target.closest !== 'function') return;
        const interactiveTarget = target.closest(
          'button, a, input, select, textarea, [contenteditable="true"], [role="button"], [role="menuitem"], [data-dashboard-no-drag]',
        );
        if (interactiveTarget) return;
        editing.onInteractionStart(event, elementId, 'move');
      }}
      onClick={(event) => {
        event.stopPropagation();
        editing.onSelectElement(
          elementId,
          event.metaKey || event.ctrlKey,
        );
      }}
      onFocus={(event) => {
        if (event.target === event.currentTarget) {
          editing.onSelectElement(elementId, false);
        }
      }}
      className={cn(
        'group/dashboard-edit relative min-h-0 min-w-0 outline-none transition-colors focus-visible:bg-accent/[0.04]',
        element.kind === 'panel' && 'touch-none select-none cursor-grab',
        selected && 'bg-accent/[0.06]',
      )}
      style={style}
    >
      {children}
      {selected && (
        <span
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 z-10 bg-accent/[0.04]"
        />
      )}
      <span
        aria-hidden="true"
        data-dashboard-no-drag=""
        data-dashboard-resize-handle=""
        onPointerDown={(event) =>
          editing.onInteractionStart(event, elementId, 'resize')
        }
        className="absolute bottom-0 right-0 z-20 h-3 w-3 touch-none cursor-nwse-resize"
      />
    </div>
  );
}
