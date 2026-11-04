import * as React from 'react';

import { useUiPreference } from '@/shell/uiPreference/useUiPreference';

export type ColumnDropPosition = 'before' | 'after';

function parseColumns(raw: unknown): { fields: string[] } | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null;
  const fields = (raw as Record<string, unknown>).fields;
  return Array.isArray(fields) && fields.every((field) => typeof field === 'string') ? { fields } : null;
}

/** The columns kept for a table, as far as the table still has them; the defaults where nothing is kept. */
export function settleColumns<T extends string>(kept: readonly string[] | undefined, known: readonly T[], defaults: readonly T[]): T[] {
  const settled = (kept ?? defaults).filter((id, index, all): id is T => (known as readonly string[]).includes(id) && all.indexOf(id) === index);
  return settled.length > 0 ? settled : [...defaults];
}

/** `source` moved to stand before or after `target`. */
export function moveColumn<T extends string>(columns: readonly T[], source: string, target: string, position: ColumnDropPosition): T[] {
  const moved = columns.find((id) => id === source);
  if (!moved || source === target || !columns.some((id) => id === target)) return [...columns];

  const rest = columns.filter((id) => id !== source);
  const at = rest.findIndex((id) => id === target) + (position === 'after' ? 1 : 0);

  return [...rest.slice(0, at), moved, ...rest.slice(at)];
}

/**
 * The columns of an APM table the reader arranged, kept for them as a
 * preference: which are shown, in their order. Until they arrange any, the
 * table shows its defaults.
 */
export function useApmColumns<T extends string>(table: string, known: readonly T[], defaults: readonly T[]) {
  const kept = useUiPreference('apm.columns', table, parseColumns);
  const visible = React.useMemo(() => settleColumns(kept.value?.fields, known, defaults), [defaults, kept.value, known]);

  const save = (fields: readonly T[]) => kept.save({ fields: [...fields] });

  return {
    visible,
    available: known.filter((id) => !visible.includes(id)),
    arranged: kept.value !== null,
    toggle: (id: T) => {
      if (visible.includes(id)) {
        if (visible.length > 1) save(visible.filter((column) => column !== id));
      } else {
        save([...visible, id]);
      }
    },
    reorder: (source: string, target: string, position: ColumnDropPosition) => save(moveColumn(visible, source, target, position)),
    reset: () => kept.clear(),
  };
}
