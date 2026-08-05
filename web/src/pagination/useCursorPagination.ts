import * as React from 'react';

import type { CursorLinks } from './cursor';

interface CursorPaginationOptions {
  contextKey: string;
  defaultPageSize?: number;
}

interface CursorPosition {
  contextKey: string;
  cursor: string | null;
}

/**
 * Shared client state for server keyset pagination. A context-key change makes
 * an old cursor inactive during render, before effects run, so a new filter,
 * time range, or sort can never issue a request with the previous cursor.
 */
export function useCursorPagination({
  contextKey,
  defaultPageSize = 20,
}: CursorPaginationOptions) {
  const [pageSize, setPageSizeState] = React.useState(defaultPageSize);
  const [position, setPosition] = React.useState<CursorPosition>({
    contextKey,
    cursor: null,
  });

  const cursor = position.contextKey === contextKey ? position.cursor : null;

  React.useEffect(() => {
    setPosition((current) =>
      current.contextKey === contextKey
        ? current
        : { contextKey, cursor: null },
    );
  }, [contextKey]);

  const reset = React.useCallback(() => {
    setPosition({ contextKey, cursor: null });
  }, [contextKey]);

  const goPrevious = React.useCallback(
    (page: CursorLinks | null | undefined) => {
      if (!page?.previous_cursor) return;
      setPosition({ contextKey, cursor: page.previous_cursor });
    },
    [contextKey],
  );

  const goNext = React.useCallback(
    (page: CursorLinks | null | undefined) => {
      if (!page?.next_cursor) return;
      setPosition({ contextKey, cursor: page.next_cursor });
    },
    [contextKey],
  );

  const setPageSize = React.useCallback(
    (nextPageSize: number) => {
      setPageSizeState(nextPageSize);
      setPosition({ contextKey, cursor: null });
    },
    [contextKey],
  );

  return {
    cursor,
    pageSize,
    reset,
    goPrevious,
    goNext,
    setPageSize,
  };
}
