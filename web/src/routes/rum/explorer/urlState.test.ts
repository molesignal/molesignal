import { describe, expect, it } from 'vitest';

import { quoteRumValue, rumQueryFromParams, rumTabFromParams, writeRumQueryParams, writeRumTabParams } from './urlState';

const params = (search: string) => new URLSearchParams(search);

describe('the address of the explorer', () => {
  it('names the tab, and opens on the first when it names none or an unknown one', () => {
    expect(rumTabFromParams(params('type=errors'))).toBe('errors');
    expect(rumTabFromParams(params(''))).toBe('sessions');
    expect(rumTabFromParams(params('type=nope'))).toBe('sessions');
  });

  it('leaves the first tab out of the address', () => {
    expect(writeRumTabParams(params('q=a'), 'views').toString()).toBe('q=a&type=views');
    expect(writeRumTabParams(params('type=views&q=a'), 'sessions').toString()).toBe('q=a');
  });

  it('carries the query in q, or in its older spelling', () => {
    expect(rumQueryFromParams(params('q=%20usr.id%20%3D%20%27u%27%20'))).toBe("usr.id = 'u'");
    expect(rumQueryFromParams(params('query=env%3D%27prod%27'))).toBe("env='prod'");
    expect(rumQueryFromParams(params(''))).toBe('');
  });

  it('writes the applied query in place of the old spelling, and no query is no parameter', () => {
    expect(writeRumQueryParams(params('query=old&type=views'), "env = 'prod'").toString()).toBe(
      'type=views&q=env+%3D+%27prod%27',
    );
    expect(writeRumQueryParams(params('q=old&type=views'), '  ').toString()).toBe('type=views');
  });

  it('quotes a value for a field query', () => {
    expect(quoteRumValue("it's a \\ path")).toBe("it\\'s a \\\\ path");
  });
});
