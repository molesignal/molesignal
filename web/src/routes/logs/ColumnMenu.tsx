import { Columns3, GripVertical, RotateCcw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
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

const TIMESTAMP_FIELDS = new Set(['_timestamp', 'timestamp', 'time']);

export type LogColumnDropPosition = 'before' | 'after';

export function reorderVisibleLogFields(
  fields: string[],
  source: string,
  target: string,
  position: LogColumnDropPosition,
): string[] {
  if (source === target || !fields.includes(source) || !fields.includes(target)) return fields;
  const reordered = fields.filter((field) => field !== source);
  const targetIndex = reordered.indexOf(target);
  if (targetIndex < 0) return fields;
  reordered.splice(targetIndex + (position === 'after' ? 1 : 0), 0, source);
  return reordered;
}

interface LogColumnMenuProps {
  fields: string[];
  visibleFields: string[];
  onToggleField: (field: string) => void;
  onReorderField: (
    source: string,
    target: string,
    position: LogColumnDropPosition,
  ) => void;
  /** The columns are the reader's own arrangement for this stream, which is kept. */
  arranged?: boolean | undefined;
  /** Puts back what the stream opens with and forgets the arrangement. */
  onReset?: (() => void) | undefined;
}

interface DropTarget {
  field: string;
  position: LogColumnDropPosition;
}

export function LogColumnMenu({
  fields,
  visibleFields,
  onToggleField,
  onReorderField,
  arranged = false,
  onReset,
}: LogColumnMenuProps) {
  const { t } = useTranslation('logs');
  const [draggingField, setDraggingField] = React.useState<string | null>(null);
  const [dropTarget, setDropTarget] = React.useState<DropTarget | null>(null);
  const availableFields = React.useMemo(
    () => Array.from(new Set(fields.filter((field) => !TIMESTAMP_FIELDS.has(field)))),
    [fields],
  );
  const availableSet = React.useMemo(() => new Set(availableFields), [availableFields]);
  const orderedVisibleFields = React.useMemo(
    () => visibleFields.filter((field) => availableSet.has(field)),
    [availableSet, visibleFields],
  );
  const visibleSet = React.useMemo(() => new Set(orderedVisibleFields), [orderedVisibleFields]);
  const hiddenFields = React.useMemo(
    () => availableFields.filter((field) => !visibleSet.has(field)),
    [availableFields, visibleSet],
  );

  const clearDragState = React.useCallback(() => {
    setDraggingField(null);
    setDropTarget(null);
  }, []);

  const moveFromKeyboard = React.useCallback((field: string, direction: -1 | 1) => {
    const index = orderedVisibleFields.indexOf(field);
    const target = orderedVisibleFields[index + direction];
    if (!target) return;
    onReorderField(field, target, direction < 0 ? 'before' : 'after');
  }, [onReorderField, orderedVisibleFields]);

  return (
    <DropdownMenu onOpenChange={(open) => {
      if (!open) clearDragState();
    }}>
      <DropdownMenuTrigger asChild>
        <QueryIconAction icon={Columns3} label={t('explore.results.columns')} data-log-columns />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        // No taller than the room under its button: a short window scrolls it instead of cutting it.
        className="max-h-[min(420px,var(--radix-dropdown-menu-content-available-height))] w-72 overflow-auto"
      >
        <DropdownMenuLabel className="flex items-center justify-between gap-3">
          <span>{t('explore.results.visible_columns')}</span>
          <span className="font-normal text-tx-3">{t('explore.results.reorder_columns_hint')}</span>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        {orderedVisibleFields.map((field) => {
          const target = dropTarget?.field === field ? dropTarget : null;
          return (
            <DropdownMenuCheckboxItem
              key={field}
              checked
              draggable
              aria-label={t('explore.results.drag_column_aria', { name: field })}
              onCheckedChange={() => onToggleField(field)}
              onSelect={(event) => event.preventDefault()}
              onDragStart={(event) => {
                event.dataTransfer.effectAllowed = 'move';
                event.dataTransfer.setData('text/plain', field);
                setDraggingField(field);
              }}
              onDragOver={(event) => {
                const source = draggingField || event.dataTransfer.getData('text/plain');
                if (!source || source === field) return;
                event.preventDefault();
                event.dataTransfer.dropEffect = 'move';
                const rect = event.currentTarget.getBoundingClientRect();
                setDropTarget({
                  field,
                  position: event.clientY < rect.top + rect.height / 2 ? 'before' : 'after',
                });
              }}
              onDrop={(event) => {
                event.preventDefault();
                const source = draggingField || event.dataTransfer.getData('text/plain');
                const position = target?.position
                  ?? (event.clientY < event.currentTarget.getBoundingClientRect().top
                    + event.currentTarget.getBoundingClientRect().height / 2
                    ? 'before'
                    : 'after');
                if (source && source !== field) onReorderField(source, field, position);
                clearDragState();
              }}
              onDragEnd={clearDragState}
              onKeyDown={(event) => {
                if (!event.altKey || (event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return;
                event.preventDefault();
                event.stopPropagation();
                moveFromKeyboard(field, event.key === 'ArrowUp' ? -1 : 1);
              }}
              className={cn(
                'group/column gap-1.5 pr-2 font-sans text-xs',
                'cursor-grab active:cursor-grabbing',
                draggingField === field && 'opacity-40',
                target?.position === 'before'
                  && 'before:absolute before:inset-x-2 before:top-0 before:h-0.5 before:rounded-full before:bg-indigo',
                target?.position === 'after'
                  && 'after:absolute after:inset-x-2 after:bottom-0 after:h-0.5 after:rounded-full after:bg-indigo',
              )}
            >
              <GripVertical aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-tx-3" />
              <span className="min-w-0 flex-1 truncate">{field}</span>
            </DropdownMenuCheckboxItem>
          );
        })}
        {/* Offered only while there is an arrangement to undo: the default has nothing to go back to. */}
        {arranged && onReset ? (
          <DropdownMenuItem data-log-columns-reset onSelect={onReset} className="font-sans text-xs text-tx-2">
            <RotateCcw aria-hidden="true" className="h-3.5 w-3.5 shrink-0" />
            {t('explore.results.reset_columns')}
          </DropdownMenuItem>
        ) : null}
        {hiddenFields.length > 0 ? (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuLabel>{t('explore.results.available_columns')}</DropdownMenuLabel>
            {hiddenFields.map((field) => (
              <DropdownMenuCheckboxItem
                key={field}
                checked={false}
                onCheckedChange={() => onToggleField(field)}
                onSelect={(event) => event.preventDefault()}
              >
                <span className="truncate font-sans text-xs">{field}</span>
              </DropdownMenuCheckboxItem>
            ))}
          </>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
