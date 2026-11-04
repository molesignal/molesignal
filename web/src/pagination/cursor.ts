export interface CursorPage<T> {
  items: T[];
  nextCursor: string | null;
  previousCursor: string | null;
  hasMore: boolean;
}

export type CursorLinks = Pick<
  CursorPage<unknown>,
  'nextCursor' | 'previousCursor'
>;
