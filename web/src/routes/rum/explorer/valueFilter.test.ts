import { describe, expect, it } from 'vitest';

import type { RumField } from '@/api/rum';

import {
  canFilterRumValue,
  hasRumValueFilter,
  insertRumValueClause,
  removeRumValueFilter,
  splitRumStatement,
} from './valueFilter';

const text: RumField = { name: 'browser.name', dataType: 'utf8', unit: 'none', facetable: true, indexed: true };
const status: RumField = { name: 'resource.status_code', dataType: 'int64', unit: 'none', facetable: true, indexed: false };
const flag: RumField = { name: 'view.is_active', dataType: 'bool', unit: 'none', facetable: true, indexed: false };
const document: RumField = { name: 'context', dataType: 'json', unit: 'none', facetable: false, indexed: false };

describe('canFilterRumValue', () => {
  it('takes a value of the type of its field', () => {
    expect(canFilterRumValue(text, 'Chrome', '=')).toBe(true);
    expect(canFilterRumValue(status, '404', '=')).toBe(true);
    expect(canFilterRumValue(status, 'ok', '=')).toBe(false);
    expect(canFilterRumValue(flag, 'true', '!=')).toBe(true);
    expect(canFilterRumValue(flag, 'yes', '!=')).toBe(false);
  });

  it('asks for bounds of numbers only', () => {
    expect(canFilterRumValue(status, '500', '>=')).toBe(true);
    expect(canFilterRumValue(text, 'Chrome', '>=')).toBe(false);
  });

  it('refuses a value the server would not match to itself, and a document', () => {
    expect(canFilterRumValue(text, '', '=')).toBe(false);
    expect(canFilterRumValue(text, ' Chrome', '=')).toBe(false);
    expect(canFilterRumValue(text, 'a'.repeat(513), '=')).toBe(false);
    expect(canFilterRumValue(document, '{}', '=')).toBe(false);
  });
});

describe('splitRumStatement', () => {
  it('splits on AND outside quotes', () => {
    expect(splitRumStatement("a = 'x AND y' AND b != 'z'  and c >= 1")).toEqual([
      "a = 'x AND y'",
      "b != 'z'",
      'c >= 1',
    ]);
    expect(splitRumStatement('')).toEqual([]);
  });
});

describe('insertRumValueClause', () => {
  it('adds the comparison to the end of the statement', () => {
    expect(insertRumValueClause('', text, 'Chrome', '=')).toBe("browser.name = 'Chrome'");
    expect(insertRumValueClause("usr.id = 'u'", status, '500', '>=')).toBe("usr.id = 'u' AND resource.status_code >= 500");
    expect(insertRumValueClause('', flag, 'true', '=')).toBe('view.is_active = true');
  });

  it('quotes a value with a quote in it', () => {
    expect(insertRumValueClause('', text, "O'Brien", '=')).toBe("browser.name = 'O\\'Brien'");
  });

  it('keeps the statement as it is when it holds the comparison already', () => {
    const current = "browser.name = 'Chrome'";

    expect(insertRumValueClause(current, text, 'Chrome', '=')).toBe(current);
  });

  it('takes the place of the comparison the field’s plus left for a value', () => {
    expect(insertRumValueClause("a = 'b' AND browser.name = ''", text, 'Chrome', '=')).toBe(
      "a = 'b' AND browser.name = 'Chrome'",
    );
  });

  it('takes the place of another value of the same field, and of the same value asked the other way', () => {
    expect(insertRumValueClause("browser.name = 'Safari'", text, 'Chrome', '=')).toBe("browser.name = 'Chrome'");
    expect(insertRumValueClause("browser.name = 'Chrome'", text, 'Chrome', '!=')).toBe("browser.name != 'Chrome'");
  });

  it('leaves other values asked out as they were, and replaces a bound on the same side only', () => {
    expect(insertRumValueClause("browser.name != 'Safari'", text, 'Chrome', '!=')).toBe(
      "browser.name != 'Safari' AND browser.name != 'Chrome'",
    );
    expect(insertRumValueClause('resource.status_code >= 400', status, '500', '>=')).toBe('resource.status_code >= 500');
    expect(insertRumValueClause('resource.status_code >= 400', status, '599', '<=')).toBe(
      'resource.status_code >= 400 AND resource.status_code <= 599',
    );
  });
});

describe('hasRumValueFilter and removeRumValueFilter', () => {
  const statement = "usr.id = 'u' AND browser.name != 'Safari'";

  it('find the comparison however it is spaced', () => {
    expect(hasRumValueFilter(statement, text, 'Safari', '!=')).toBe(true);
    expect(hasRumValueFilter("browser.name!='Safari'", text, 'Safari', '!=')).toBe(true);
    expect(hasRumValueFilter(statement, text, 'Safari', '=')).toBe(false);
    expect(hasRumValueFilter(statement, text, 'Chrome', '!=')).toBe(false);
  });

  it('take it out and leave the rest as it was written', () => {
    expect(removeRumValueFilter(statement, text, 'Safari', '!=')).toBe("usr.id = 'u'");
    expect(removeRumValueFilter(statement, text, 'Chrome', '!=')).toBe(statement);
  });
});
