import { describe, expect, it } from 'vitest';

import { shouldShowStatusPagePagination } from './pagination';

describe('shouldShowStatusPagePagination', () => {
  it('hides pagination until the result needs a second page', () => {
    expect(shouldShowStatusPagePagination(undefined)).toBe(false);
    expect(shouldShowStatusPagePagination({ total: 0, per_page: 25 })).toBe(false);
    expect(shouldShowStatusPagePagination({ total: 25, per_page: 25 })).toBe(false);
    expect(shouldShowStatusPagePagination({ total: 26, per_page: 25 })).toBe(true);
  });
});
