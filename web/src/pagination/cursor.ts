export interface CursorPage<T> {
  items: T[];
  next_cursor: string | null;
  previous_cursor: string | null;
  has_more: boolean;
}

export type CursorLinks = Pick<
  CursorPage<unknown>,
  'next_cursor' | 'previous_cursor'
>;
