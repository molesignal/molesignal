import { describe, expect, it } from 'vitest';

import type { LogFieldDef } from './fieldQueryModel';
import {
  appendPinnedFilters,
  appendSqlFieldContainsFilter,
  appendSqlFieldFilter,
  appendSqlFieldValueFilter,
} from './sqlStatement';

const EITHER = `SELECT * FROM "app"\nWHERE level = 'error' OR level = 'warn'\nORDER BY _timestamp DESC\nLIMIT 100`;
const NARROWED = (condition: string) =>
  `SELECT * FROM "app"\nWHERE (level = 'error' OR level = 'warn')\n  AND ${condition}\nORDER BY _timestamp DESC\nLIMIT 100`;

const field = (name: string, dataType: LogFieldDef['dataType']): LogFieldDef => ({
  name,
  type: 'string',
  dataType,
  count: '0',
  expandable: false,
});

describe('a SQL statement of Logs', () => {
  it('takes the pinned filters on all of its alternatives', () => {
    expect(
      appendPinnedFilters(EITHER, 'app', [
        { field: 'service', operator: '=', value: 'api' },
        { field: 'env', operator: '!=', value: 'dev' },
      ]),
    ).toBe(
      `SELECT * FROM "app"\nWHERE (level = 'error' OR level = 'warn')\n  AND "service" = 'api'\n  AND "env" != 'dev'\nORDER BY _timestamp DESC\nLIMIT 100`,
    );
  });

  it('takes a value, a fragment or a field from the field list on all of its alternatives', () => {
    expect(appendSqlFieldValueFilter(EITHER, 'app', 'service', 'api', 'include')).toBe(NARROWED(`"service" = 'api'`));
    expect(appendSqlFieldValueFilter(EITHER, 'app', 'service', 'api', 'exclude')).toBe(NARROWED(`"service" != 'api'`));
    expect(appendSqlFieldContainsFilter(EITHER, 'app', 'message', 'timeout')).toBe(NARROWED(`"message" LIKE '%timeout%'`));
    expect(appendSqlFieldFilter(EITHER, 'app', field('status', 'int64'))).toBe(NARROWED('"status" >= 0'));
  });

  it('starts a WHERE where there is none, before the order and the limit', () => {
    expect(appendSqlFieldFilter('', 'app', field('service', 'utf8'))).toBe(
      `SELECT * FROM "app"\nWHERE "service" = ''\nORDER BY _timestamp DESC\nLIMIT 200`,
    );
  });

  it('adds to a statement that groups before its grouping', () => {
    const grouped = 'SELECT level, COUNT(*) AS events FROM "app"\nGROUP BY level\nORDER BY events DESC';

    expect(appendPinnedFilters(grouped, 'app', [{ field: 'service', operator: '=', value: 'api' }])).toBe(
      `SELECT level, COUNT(*) AS events FROM "app"\nWHERE "service" = 'api'\nGROUP BY level\nORDER BY events DESC`,
    );
  });
});
