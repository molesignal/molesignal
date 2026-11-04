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

import type { ColumnDropPosition } from './useApmColumns';

interface ApmColumnsHandle<T extends string> {
  visible: readonly T[];
  available: readonly T[];
  arranged: boolean;
  toggle: (id: T) => void;
  reorder: (source: string, target: string, position: ColumnDropPosition) => void;
  reset: () => void;
}

/**
 * The columns of an APM table, set as the columns of the trace table are:
 * the ones shown, ticked, in the order they stand, dragged (or moved with Alt
 * and an arrow) to another place; the hidden ones to add.
 */
export function ApmColumnMenu<T extends string>({ columns, label }: { columns: ApmColumnsHandle<T>; label: (id: T) => string }) {
  const { t } = useTranslation('apm');
  const [dragging, setDragging] = React.useState<string | null>(null);
  const [target, setTarget] = React.useState<{
    id: string;
    position: ColumnDropPosition;
  } | null>(null);

  const clear = () => {
    setDragging(null);
    setTarget(null);
  };
  // A moved column slides to its new place in the list.
  const listRef = React.useRef<HTMLDivElement>(null);
  const captureOrder = useFlipList(listRef, columns.visible.join('\n'));

  const positionOf = (event: React.DragEvent<HTMLElement>): ColumnDropPosition => {
    const rect = event.currentTarget.getBoundingClientRect();
    return event.clientY < rect.top + rect.height / 2 ? 'before' : 'after';
  };

  return (
    <DropdownMenu onOpenChange={(open) => (open ? undefined : clear())}>
      <DropdownMenuTrigger asChild>
        <QueryIconAction icon={Columns3} label={t('columns.title')} size="sm" data-apm-columns />
      </DropdownMenuTrigger>
      <DropdownMenuContent ref={listRef} align="end" className="max-h-[min(460px,var(--radix-dropdown-menu-content-available-height))] w-64 overflow-auto">
        <DropdownMenuLabel className="flex items-center justify-between gap-3">
          <span>{t('columns.visible')}</span>
          <span className="font-body text-tx-3">{t('columns.reorder_hint')}</span>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        {columns.visible.map((id, index) => {
          const here = target?.id === id ? target.position : null;

          return (
            <DropdownMenuCheckboxItem
              key={id}
              data-flip-key={id}
              checked
              draggable
              data-apm-column={id}
              disabled={columns.visible.length === 1}
              aria-label={t('columns.drag_aria', { name: label(id) })}
              onCheckedChange={() => columns.toggle(id)}
              onSelect={(event) => event.preventDefault()}
              onDragStart={(event) => {
                event.dataTransfer.effectAllowed = 'move';
                event.dataTransfer.setData('text/plain', id);
                setDragging(id);
              }}
              onDragOver={(event) => {
                const source = dragging || event.dataTransfer.getData('text/plain');
                if (!source || source === id) return;
                event.preventDefault();
                setTarget({ id, position: positionOf(event) });
              }}
              onDrop={(event) => {
                event.preventDefault();
                const source = dragging || event.dataTransfer.getData('text/plain');
                if (source && source !== id) {
                  captureOrder();
                  columns.reorder(source, id, here ?? positionOf(event));
                }
                clear();
              }}
              onDragEnd={clear}
              onKeyDown={(event) => {
                if (!event.altKey || (event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return;
                event.preventDefault();
                const neighbour = columns.visible[index + (event.key === 'ArrowUp' ? -1 : 1)];
                if (!neighbour) return;
                captureOrder();
                columns.reorder(id, neighbour, event.key === 'ArrowUp' ? 'before' : 'after');
              }}
              className={cn(
                'gap-1.5 pr-2 font-sans text-xs cursor-grab active:cursor-grabbing',
                dragging === id && 'opacity-40',
                here === 'before' && 'before:absolute before:inset-x-2 before:top-0 before:h-0.5 before:rounded-full before:bg-indigo',
                here === 'after' && 'after:absolute after:inset-x-2 after:bottom-0 after:h-0.5 after:rounded-full after:bg-indigo',
              )}
            >
              <GripVertical aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
              <span className="min-w-0 flex-1 truncate">{label(id)}</span>
            </DropdownMenuCheckboxItem>
          );
        })}
        {columns.arranged && (
          <DropdownMenuItem data-apm-columns-reset onSelect={columns.reset} className="font-sans text-xs text-tx-2">
            <RotateCcw aria-hidden className="h-3.5 w-3.5 shrink-0" />
            {t('columns.reset')}
          </DropdownMenuItem>
        )}
        {columns.available.length > 0 && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuLabel>{t('columns.available')}</DropdownMenuLabel>
            {columns.available.map((id) => (
              <DropdownMenuCheckboxItem
                key={id}
                checked={false}
                data-apm-column={id}
                onCheckedChange={() => columns.toggle(id)}
                onSelect={(event) => event.preventDefault()}
              >
                <span className="truncate font-sans text-xs">{label(id)}</span>
              </DropdownMenuCheckboxItem>
            ))}
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
