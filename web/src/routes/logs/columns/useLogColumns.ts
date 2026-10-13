import * as React from 'react';

import { useUiPreference } from '@/shell/uiPreference/useUiPreference';

import { type LogColumnDropPosition, reorderVisibleLogFields } from '../ColumnMenu';
import {
  captureLogFieldPosition,
  defaultLogTableFields,
  type LogColumnCandidate,
  type LogFieldPosition,
  restoreLogFieldPosition,
} from '../viewModel';

/** The purpose a stream's columns are kept under, keyed by the stream's name. */
export const LOG_COLUMNS_SCOPE = 'logs.columns';

/** An arrangement as it is kept: the fields beside the time, in their order. */
export interface KeptLogColumns {
  fields: string[];
}

/** Reads a kept arrangement; anything else is nothing kept. */
export function parseKeptLogColumns(raw: unknown): KeptLogColumns | null {
  if (!raw || typeof raw !== 'object') return null;

  const fields = (raw as { fields?: unknown }).fields;
  if (!Array.isArray(fields) || !fields.every((field) => typeof field === 'string')) return null;

  return { fields: [...new Set(fields as string[])] };
}

/** A field of the stream with how many of the loaded logs have a value for it. */
export interface LogColumnField extends LogColumnCandidate {
  count?: string | number | undefined;
}

/**
 * Where the columns on screen came from: what the stream opens with, what was
 * kept for it, or what the reader has arranged on this page since.
 */
export type LogColumnsOrigin = 'default' | 'kept' | 'reader';

export interface LogColumnsState {
  stream: string;
  origin: LogColumnsOrigin;
  visibleFields: string[];
  /** Where a column the reader hid stood, so that showing it again puts it back there. */
  hiddenPositions: Record<string, LogFieldPosition>;
}

export const NO_LOG_COLUMNS: LogColumnsState = {
  stream: '',
  origin: 'default',
  visibleFields: [],
  hiddenPositions: {},
};

const sameNames = (a: readonly string[], b: readonly string[]) =>
  a.length === b.length && a.every((name, index) => name === b[index]);

/**
 * The columns for `stream`, given what is on screen, what the stream has and
 * what is kept for it.
 *
 * What the reader arranged on this page stands. Otherwise what is kept for the
 * stream is shown, as far as the stream still has those fields; an arrangement
 * none of whose fields is left counts as none. Otherwise the stream opens with
 * its default, which is chosen once and not again as more logs load.
 */
export function settleLogColumns(
  current: LogColumnsState,
  stream: string,
  fields: readonly LogColumnField[],
  kept: KeptLogColumns | null,
): LogColumnsState {
  const sameStream = current.stream === stream;

  if (fields.length === 0) {
    const empty = sameStream
      && current.visibleFields.length === 0
      && Object.keys(current.hiddenPositions).length === 0;

    return empty ? current : { ...NO_LOG_COLUMNS, stream };
  }

  const known = new Set(fields.map((field) => field.name));
  const stillThere = (names: readonly string[]) => names.filter((name) => known.has(name));
  const settled = (origin: LogColumnsOrigin, visibleFields: string[]): LogColumnsState => {
    const hiddenPositions = sameStream
      ? Object.fromEntries(Object.entries(current.hiddenPositions).filter(([name]) => known.has(name)))
      : {};
    const unchanged = sameStream
      && current.origin === origin
      && sameNames(current.visibleFields, visibleFields)
      && Object.keys(hiddenPositions).length === Object.keys(current.hiddenPositions).length;

    return unchanged ? current : { stream, origin, visibleFields, hiddenPositions };
  };

  if (sameStream && current.origin === 'reader') return settled('reader', stillThere(current.visibleFields));

  if (kept) {
    const fromKept = stillThere(kept.fields);
    // No column at all is an arrangement; columns the stream no longer has are not.
    if (fromKept.length > 0 || kept.fields.length === 0) return settled('kept', fromKept);
  }

  if (sameStream && current.origin === 'default') {
    const chosen = stillThere(current.visibleFields);
    if (chosen.length > 0) return settled('default', chosen);
  }

  // A column opens on what is there: the fields the loaded logs have a value for, where any are loaded.
  const populated = fields.filter((field) => Number(field.count) > 0);

  return settled('default', defaultLogTableFields(populated.length > 0 ? populated : fields));
}

export interface LogColumns {
  visibleFields: string[];
  /** The reader has an arrangement of their own for this stream. */
  arranged: boolean;
  toggle: (field: string) => void;
  reorder: (source: string, target: string, position: LogColumnDropPosition) => void;
  /** Back to what the stream opens with; what was kept for it is forgotten. */
  reset: () => void;
}

/**
 * The columns a log stream is listed in, and the reader's own arrangement of
 * them: which fields stand beside the time, in what order.
 *
 * An arrangement is kept per stream, for the reader alone, as soon as it is
 * made: on the server, so that it is the same in another browser, and in this
 * one, so that the page opens on it at once.
 */
export function useLogColumns(stream: string, fields: readonly LogColumnField[]): LogColumns {
  const kept = useUiPreference(LOG_COLUMNS_SCOPE, stream || null, parseKeptLogColumns);
  const [state, setState] = React.useState<LogColumnsState>(NO_LOG_COLUMNS);
  const stateRef = React.useRef(state);
  stateRef.current = state;

  React.useEffect(() => {
    setState((current) => settleLogColumns(current, stream, fields, kept.value));
  }, [fields, kept.value, stream]);

  const { save, clear } = kept;
  const arrange = React.useCallback(
    (change: (current: LogColumnsState) => Pick<LogColumnsState, 'visibleFields' | 'hiddenPositions'>) => {
      const current = stateRef.current;
      const next: LogColumnsState = { ...current, ...change(current), origin: 'reader' };

      stateRef.current = next;
      setState(next);
      save({ fields: next.visibleFields });
    },
    [save],
  );

  const toggle = React.useCallback(
    (field: string) => arrange((current) => {
      if (current.visibleFields.includes(field)) {
        const position = captureLogFieldPosition(current.visibleFields, field);

        return {
          visibleFields: current.visibleFields.filter((name) => name !== field),
          hiddenPositions: position ? { ...current.hiddenPositions, [field]: position } : current.hiddenPositions,
        };
      }

      const hiddenPositions = { ...current.hiddenPositions };
      const position = hiddenPositions[field];
      delete hiddenPositions[field];

      return {
        visibleFields: restoreLogFieldPosition(current.visibleFields, field, position),
        hiddenPositions,
      };
    }),
    [arrange],
  );

  const reorder = React.useCallback(
    (source: string, target: string, position: LogColumnDropPosition) => arrange((current) => ({
      visibleFields: reorderVisibleLogFields(current.visibleFields, source, target, position),
      hiddenPositions: current.hiddenPositions,
    })),
    [arrange],
  );

  const reset = React.useCallback(() => {
    clear();
    // Nothing chosen and nothing kept: the next settling opens the stream with its default.
    const next = { ...NO_LOG_COLUMNS, stream: stateRef.current.stream };
    stateRef.current = next;
    setState(next);
  }, [clear]);

  return React.useMemo(
    () => ({
      visibleFields: state.visibleFields,
      arranged: state.origin !== 'default',
      toggle,
      reorder,
      reset,
    }),
    [reorder, reset, state.origin, state.visibleFields, toggle],
  );
}
