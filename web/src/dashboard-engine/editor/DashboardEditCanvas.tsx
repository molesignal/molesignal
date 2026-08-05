import {
  ClipboardPaste,
  Copy,
  Download,
  Trash2,
  WandSparkles,
} from 'lucide-react';
import * as React from 'react';

import { ChromeButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';

import { DashboardRenderer } from '../DashboardRenderer';
import { useDashboardText } from '../i18n';
import { autoLayout, placeLayoutItem } from '../layout';
import type { DashboardDefinition, DashboardElement } from '../schema';
import type {
  DashboardGridEditingConfig,
  DashboardGridInteractionMode,
} from './EditableDashboardGridItem';

interface GridInteraction {
  mode: DashboardGridInteractionMode;
  id: string;
  pointerId: number;
  startX: number;
  startY: number;
  columnWidth: number;
  rowStep: number;
  originals: DashboardElement[];
  selected: Set<string>;
  changed: boolean;
}

interface DashboardEditCanvasProps {
  definition: DashboardDefinition;
  orgId: string;
  selectedIds: Set<string>;
  clipboardSize: number;
  onSelect: (ids: Set<string>) => void;
  onOpenPanel: (panelId: string) => void;
  onCommitElements: (elements: DashboardElement[]) => void;
  onCopy: () => void;
  onPaste: () => void;
  onDuplicateElement: (elementId: string) => void;
  onRemoveElement: (elementId: string) => void;
  onExport: () => void;
}

export function DashboardEditCanvas({
  definition,
  orgId,
  selectedIds,
  clipboardSize,
  onSelect,
  onOpenPanel,
  onCommitElements,
  onCopy,
  onPaste,
  onDuplicateElement,
  onRemoveElement,
  onExport,
}: DashboardEditCanvasProps) {
  const tr = useDashboardText();
  const interactionRef = React.useRef<GridInteraction | null>(null);
  const cleanupInteractionRef = React.useRef<null | (() => void)>(null);
  const previewRef = React.useRef(definition.elements);
  const [preview, setPreview] = React.useState(definition.elements);

  React.useEffect(() => {
    if (!interactionRef.current) {
      previewRef.current = definition.elements;
      setPreview(definition.elements);
    }
  }, [definition.elements]);

  React.useEffect(
    () => () => {
      cleanupInteractionRef.current?.();
    },
    [],
  );

  const selectElement = React.useCallback(
    (elementId: string, additive: boolean) => {
      if (!additive) {
        onSelect(new Set([elementId]));
        return;
      }
      const next = new Set(selectedIds);
      if (next.has(elementId)) next.delete(elementId);
      else next.add(elementId);
      onSelect(next);
    },
    [onSelect, selectedIds],
  );

  const beginInteraction = React.useCallback(
    (
      event: React.PointerEvent<HTMLElement>,
      elementId: string,
      mode: DashboardGridInteractionMode,
    ) => {
      event.preventDefault();
      event.stopPropagation();
      const grid = event.currentTarget.closest<HTMLElement>(
        '[data-dashboard-editor-grid]',
      );
      if (!grid) return;
      const target = definition.elements.find(
        (element) => element.id === elementId,
      );
      if (!target) return;
      const rect = grid.getBoundingClientRect();
      const selected =
        selectedIds.has(elementId) && mode === 'move'
          ? new Set(selectedIds)
          : new Set([elementId]);
      onSelect(selected);
      const interaction: GridInteraction = {
        mode,
        id: elementId,
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        columnWidth:
          (rect.width -
            definition.layout.gap * (definition.layout.columns - 1)) /
          definition.layout.columns,
        rowStep: definition.layout.rowHeight + definition.layout.gap,
        originals: definition.elements.map((element) =>
          globalThis.structuredClone(element),
        ),
        selected,
        changed: false,
      };
      interactionRef.current = interaction;
      previewRef.current = interaction.originals;

      const update = (pointerEvent: PointerEvent) => {
        if (pointerEvent.pointerId !== interaction.pointerId) return;
        pointerEvent.preventDefault();
        const dx = Math.round(
          (pointerEvent.clientX - interaction.startX) /
            (interaction.columnWidth + definition.layout.gap),
        );
        const dy = Math.round(
          (pointerEvent.clientY - interaction.startY) /
            interaction.rowStep,
        );
        const source = interaction.originals.find(
          (element) => element.id === interaction.id,
        );
        if (!source) return;
        let next = interaction.originals.map((element) =>
          globalThis.structuredClone(element),
        );
        if (interaction.mode === 'resize') {
          next = placeLayoutItem(
            next,
            source.id,
            {
              ...source.gridPos,
              w: source.gridPos.w + dx,
              h: source.gridPos.h + dy,
            },
            definition.layout.columns,
          );
        } else {
          for (const selectedId of interaction.selected) {
            const selectedSource = interaction.originals.find(
              (element) => element.id === selectedId,
            );
            if (!selectedSource) continue;
            next = placeLayoutItem(
              next,
              selectedId,
              {
                ...selectedSource.gridPos,
                x: selectedSource.gridPos.x + dx,
                y: selectedSource.gridPos.y + dy,
              },
              definition.layout.columns,
            );
          }
        }
        interaction.changed = next.some((element, index) => {
          const original = interaction.originals[index];
          return (
            !original ||
            element.gridPos.x !== original.gridPos.x ||
            element.gridPos.y !== original.gridPos.y ||
            element.gridPos.w !== original.gridPos.w ||
            element.gridPos.h !== original.gridPos.h
          );
        });
        previewRef.current = next;
        setPreview(next);
      };
      const cleanup = () => {
        globalThis.removeEventListener('pointermove', update);
        globalThis.removeEventListener('pointerup', finish);
        globalThis.removeEventListener('pointercancel', finish);
        document.body.style.removeProperty('cursor');
        document.body.style.removeProperty('user-select');
        cleanupInteractionRef.current = null;
      };
      const finish = (pointerEvent: PointerEvent) => {
        if (pointerEvent.pointerId !== interaction.pointerId) return;
        cleanup();
        interactionRef.current = null;
        if (interaction.changed) {
          onCommitElements(previewRef.current);
        } else {
          previewRef.current = definition.elements;
          setPreview(definition.elements);
        }
      };
      cleanupInteractionRef.current?.();
      cleanupInteractionRef.current = cleanup;
      document.body.style.cursor =
        mode === 'move' ? 'grabbing' : 'nwse-resize';
      document.body.style.userSelect = 'none';
      globalThis.addEventListener('pointermove', update, { passive: false });
      globalThis.addEventListener('pointerup', finish);
      globalThis.addEventListener('pointercancel', finish);
      try {
        event.currentTarget.setPointerCapture(event.pointerId);
      } catch {
        // Window-level listeners keep the edit interaction active.
      }
    },
    [definition.elements, definition.layout, onCommitElements, onSelect, selectedIds],
  );

  const nudgeSelection = React.useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      if (
        !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(
          event.key,
        ) ||
        selectedIds.size === 0
      ) {
        return;
      }
      event.preventDefault();
      const amount = event.shiftKey ? 4 : 1;
      const deltaX =
        event.key === 'ArrowLeft'
          ? -amount
          : event.key === 'ArrowRight'
            ? amount
            : 0;
      const deltaY =
        event.key === 'ArrowUp'
          ? -amount
          : event.key === 'ArrowDown'
            ? amount
            : 0;
      let next = definition.elements;
      for (const elementId of selectedIds) {
        const element = next.find((candidate) => candidate.id === elementId);
        if (!element) continue;
        next = placeLayoutItem(
          next,
          elementId,
          {
            ...element.gridPos,
            x: element.gridPos.x + deltaX,
            y: element.gridPos.y + deltaY,
          },
          definition.layout.columns,
        );
      }
      onCommitElements(next);
    },
    [definition.elements, definition.layout.columns, onCommitElements, selectedIds],
  );

  const previewDefinition = React.useMemo(
    () => ({ ...definition, elements: preview }),
    [definition, preview],
  );
  const editing = React.useMemo<DashboardGridEditingConfig>(
    () => ({
      selectedIds,
      onSelectElement: selectElement,
      onInteractionStart: beginInteraction,
    }),
    [beginInteraction, selectElement, selectedIds],
  );
  const selectedElementId = [...selectedIds][0];

  return (
    <section className="min-h-0 overflow-auto p-4">
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <div>
          <div className="font-sans text-xs font-semibold text-tx-1">
            {tr('Editing dashboard')}
          </div>
          <div className="font-mono text-type-micro text-tx-3">
            {definition.layout.columns} {tr('columns')} ·{' '}
            {definition.elements.length} {tr('elements')}
          </div>
        </div>
        <div className="ml-auto flex flex-wrap items-center justify-end gap-1">
          <ChromeButton
            onClick={() =>
              onCommitElements(
                autoLayout(
                  definition.elements,
                  definition.layout.columns,
                ),
              )
            }
          >
            <WandSparkles className="h-3.5 w-3.5" /> {tr('Auto layout')}
          </ChromeButton>
          <CopyIconButton
            disabled={selectedIds.size === 0}
            onClick={onCopy}
            label={tr('Copy')}
          />
          <ChromeButton disabled={clipboardSize === 0} onClick={onPaste}>
            <ClipboardPaste className="h-3.5 w-3.5" /> {tr('Paste')}
          </ChromeButton>
          <ChromeButton
            disabled={selectedIds.size !== 1}
            onClick={() => {
              if (selectedElementId) onDuplicateElement(selectedElementId);
            }}
          >
            <Copy className="h-3.5 w-3.5" /> {tr('Duplicate')}
          </ChromeButton>
          <ChromeButton
            disabled={selectedIds.size === 0}
            onClick={() =>
              [...selectedIds].forEach(onRemoveElement)
            }
          >
            <Trash2 className="h-3.5 w-3.5" /> {tr('Remove')}
          </ChromeButton>
          <ChromeButton onClick={onExport}>
            <Download className="h-3.5 w-3.5" /> JSON
          </ChromeButton>
        </div>
      </div>

      <div
        tabIndex={0}
        aria-label={tr('Dashboard edit canvas')}
        onKeyDown={nudgeSelection}
        onClick={() => onSelect(new Set())}
        className="min-h-[48vh] rounded-md bg-bg-0 p-2 outline-none transition-colors focus-visible:bg-bg-1/40"
      >
        <DashboardRenderer
          dashboard={previewDefinition}
          orgId={orgId}
          editMode={editing}
          onEditPanel={onOpenPanel}
          onDuplicatePanel={onDuplicateElement}
          onRemovePanel={onRemoveElement}
        />
      </div>
    </section>
  );
}
