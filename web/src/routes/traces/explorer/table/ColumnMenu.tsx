import { Columns3, GripVertical, RotateCcw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { useFlipList } from '@/shell/polish/useFlipList';
import { QueryIconAction } from '@/shell/query/IconAction';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

import { traceColumnKey, type TraceTableColumn } from './columns';
import { MAX_TRACE_COLUMNS, type TraceColumnDropPosition } from './useTraceColumns';

interface DropTarget {
  id: string;
  position: TraceColumnDropPosition;
}

interface TraceColumnMenuProps {
  columns: readonly TraceTableColumn[];
  available: { columns: readonly string[]; fields: readonly string[] };
  full: boolean;
  /** The columns are the reader's own arrangement for this stream, which is kept. */
  arranged: boolean;
  onToggle: (id: string) => void;
  onReorder: (source: string, target: string, position: TraceColumnDropPosition) => void;
  onReset: () => void;
}

/**
 * The columns of the trace table, set as the columns of the log table are:
 * the ones shown, ticked, in the order they stand, dragged (or moved with
 * Alt and an arrow) to another place; the columns a trace carries that are
 * hidden, and the fields of the root span, to add. What is set is kept for
 * the stream, and can be put back to what the table opens with.
 */
export function TraceColumnMenu({ columns, available, full, arranged, onToggle, onReorder, onReset }: TraceColumnMenuProps) {
  const { t } = useTranslation('traces');
  const [dragging, setDragging] = React.useState<string | null>(null);
  const [dropTarget, setDropTarget] = React.useState<DropTarget | null>(null);
  const label = (id: string) => {
    const key = traceColumnKey(id);
    return key ? t(`explore.table.columns.${key}`) : id;
  };

  const clear = React.useCallback(() => {
    setDragging(null);
    setDropTarget(null);
  }, []);
  // A moved column slides to its new place in the list.
  const listRef = React.useRef<HTMLDivElement>(null);
  const captureOrder = useFlipList(listRef, columns.map((column) => column.id).join('\n'));

  const moveFromKeyboard = (id: string, direction: -1 | 1) => {
    const index = columns.findIndex((column) => column.id === id);
    const target = columns[index + direction];
    if (!target) return;
    captureOrder();
    onReorder(id, target.id, direction < 0 ? 'before' : 'after');
  };

  const positionOf = (event: React.DragEvent<HTMLElement>): TraceColumnDropPosition => {
    const rect = event.currentTarget.getBoundingClientRect();
    return event.clientY < rect.top + rect.height / 2 ? 'before' : 'after';
  };

  return (
    <DropdownMenu onOpenChange={(open) => (open ? undefined : clear())}>
      <DropdownMenuTrigger asChild>
        {/* The size of the histogram's switch beside it: the head is as tall in either view. */}
        <QueryIconAction icon={Columns3} label={t('explore.columns.title')} size="sm" data-trace-columns />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        ref={listRef}
        align="end"
        // No taller than the room under its button: a short window scrolls it instead of cutting it.
        className="max-h-[min(460px,var(--radix-dropdown-menu-content-available-height))] w-72 overflow-auto"
      >
        <DropdownMenuLabel className="flex items-center justify-between gap-3">
          <span>{t('explore.columns.visible')}</span>
          <span className="font-body text-tx-3">{t('explore.columns.reorder_hint')}</span>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        {columns.map((column) => {
          const target = dropTarget?.id === column.id ? dropTarget : null;

          return (
            <DropdownMenuCheckboxItem
              key={column.id}
              data-flip-key={column.id}
              checked
              draggable
              data-trace-column={column.id}
              disabled={columns.length === 1}
              aria-label={t('explore.columns.drag_aria', { name: label(column.id) })}
              onCheckedChange={() => onToggle(column.id)}
              onSelect={(event) => event.preventDefault()}
              onDragStart={(event) => {
                event.dataTransfer.effectAllowed = 'move';
                event.dataTransfer.setData('text/plain', column.id);
                setDragging(column.id);
              }}
              onDragOver={(event) => {
                const source = dragging || event.dataTransfer.getData('text/plain');
                if (!source || source === column.id) return;
                event.preventDefault();
                event.dataTransfer.dropEffect = 'move';
                setDropTarget({ id: column.id, position: positionOf(event) });
              }}
              onDrop={(event) => {
                event.preventDefault();
                const source = dragging || event.dataTransfer.getData('text/plain');
                if (source && source !== column.id) {
                  captureOrder();
                  onReorder(source, column.id, target?.position ?? positionOf(event));
                }
                clear();
              }}
              onDragEnd={clear}
              onKeyDown={(event) => {
                if (!event.altKey || (event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return;
                event.preventDefault();
                moveFromKeyboard(column.id, event.key === 'ArrowUp' ? -1 : 1);
              }}
              className={cn(
                'group/column gap-1.5 pr-2 font-sans text-xs',
                'cursor-grab active:cursor-grabbing',
                dragging === column.id && 'opacity-40',
                target?.position === 'before' && 'before:absolute before:inset-x-2 before:top-0 before:h-0.5 before:rounded-full before:bg-indigo',
                target?.position === 'after' && 'after:absolute after:inset-x-2 after:bottom-0 after:h-0.5 after:rounded-full after:bg-indigo',
              )}
            >
              <GripVertical aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-tx-3" />
              <span className="min-w-0 flex-1 truncate">{label(column.id)}</span>
            </DropdownMenuCheckboxItem>
          );
        })}
        {/* Offered only while there is an arrangement to undo: the default has nothing to go back to. */}
        {arranged && (
          <DropdownMenuItem data-trace-columns-reset onSelect={onReset} className="font-sans text-xs text-tx-2">
            <RotateCcw aria-hidden="true" className="h-3.5 w-3.5 shrink-0" />
            {t('explore.columns.reset')}
          </DropdownMenuItem>
        )}
        {(available.columns.length > 0 || available.fields.length > 0) && <DropdownMenuSeparator />}
        {full && (available.columns.length > 0 || available.fields.length > 0) && (
          <p className="px-2 py-1 font-sans text-xs text-tx-3">{t('explore.columns.full', { count: MAX_TRACE_COLUMNS })}</p>
        )}
        {available.columns.length > 0 && (
          <>
            <DropdownMenuLabel>{t('explore.columns.available')}</DropdownMenuLabel>
            {available.columns.map((id) => (
              <AddItem key={id} id={id} label={label(id)} disabled={full} onToggle={onToggle} />
            ))}
          </>
        )}
        {available.fields.length > 0 && (
          <>
            <DropdownMenuLabel>{t('explore.columns.root_fields')}</DropdownMenuLabel>
            {available.fields.map((name) => (
              <AddItem key={name} id={name} label={name} disabled={full} onToggle={onToggle} />
            ))}
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function AddItem({ id, label, disabled, onToggle }: { id: string; label: string; disabled: boolean; onToggle: (id: string) => void }) {
  return (
    <DropdownMenuCheckboxItem
      checked={false}
      disabled={disabled}
      data-trace-column={id}
      onCheckedChange={() => onToggle(id)}
      onSelect={(event) => event.preventDefault()}
    >
      <span className="truncate font-sans text-xs">{label}</span>
    </DropdownMenuCheckboxItem>
  );
}
