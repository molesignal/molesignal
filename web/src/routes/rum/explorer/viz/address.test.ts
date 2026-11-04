import { describe, expect, it } from 'vitest';

import {
  clearRumVizFields,
  copyRumVizParams,
  readRumVizAddress,
  rumVizFromParams,
  writeRumVizParams,
  writeRumVizSettings,
} from './address';

const params = (query: string) => new URLSearchParams(query);

describe('the way of drawing in the address', () => {
  it('is the list unless the address names another', () => {
    expect(rumVizFromParams(params(''))).toBe('list');
    expect(rumVizFromParams(params('viz=timeseries'))).toBe('timeseries');
    expect(rumVizFromParams(params('viz=pie'))).toBe('list');
  });

  it('leaves the list out of the address it writes', () => {
    expect(writeRumVizParams(params('type=views'), 'table').toString()).toBe('type=views&viz=table');
    expect(writeRumVizParams(params('viz=table&type=views'), 'list').toString()).toBe('type=views');
  });
});

describe('the numbers asked, in the address', () => {
  it('are read as written', () => {
    expect(readRumVizAddress(params('m=count&m=p75:view.performance.lcp.timestamp&by=browser.name&bucket=1h&top=20&sort=count&dir=asc'))).toEqual({
      measures: ['count', 'p75:view.performance.lcp.timestamp'],
      groups: ['browser.name'],
      bucket: '1h',
      top: 20,
      sort: 'count',
      descending: false,
      field: null,
    });
    expect(readRumVizAddress(params(''))).toEqual({ measures: [], groups: [], bucket: null, top: null, sort: null, descending: null, field: null });
    expect(readRumVizAddress(params('top=-3&dir=sideways')).top).toBeNull();
    expect(readRumVizAddress(params('top=-3&dir=sideways')).descending).toBeNull();
  });

  it('are written over what was there, and only what is named', () => {
    const next = writeRumVizSettings(params('viz=table&m=count&by=a&by=b&top=5'), { groups: ['c'], bucket: '5m', descending: true });

    expect(next.getAll('m')).toEqual(['count']);
    expect(next.getAll('by')).toEqual(['c']);
    expect(next.get('bucket')).toBe('5m');
    expect(next.get('top')).toBe('5');
    expect(next.get('dir')).toBe('desc');
  });

  it('are taken out when they are emptied', () => {
    const next = writeRumVizSettings(params('m=count&by=a&bucket=5m&top=5&sort=count&dir=asc'), {
      measures: [],
      groups: [],
      bucket: null,
      top: null,
      sort: null,
      descending: null,
    });

    expect(next.toString()).toBe('');
  });

  it('leave the page\'s own parameters as they were', () => {
    const next = writeRumVizSettings(params('type=views&q=a%20%3D%20b&viz=table'), { top: 20 });

    expect(next.get('type')).toBe('views');
    expect(next.get('q')).toBe('a = b');
    expect(next.get('viz')).toBe('table');
  });
});

describe('leaving a type', () => {
  it('takes with it the fields of the type that was left, not the way of drawing or the bucket', () => {
    const next = clearRumVizFields(params('viz=table&m=count&by=a&sort=count&dir=asc&bucket=1h&top=20&q=x'));

    expect(next.toString()).toBe('viz=table&bucket=1h&top=20&q=x');
  });
});

describe('a link to the page', () => {
  it('carries how the rows are drawn when it is not the list', () => {
    const to = params('type=views');
    copyRumVizParams(params('viz=timeseries&m=count&by=a&by=b&bucket=1h&top=5&other=1'), to);

    expect(to.toString()).toBe('type=views&viz=timeseries&m=count&by=a&by=b&bucket=1h&top=5');
  });

  it('carries nothing of it for the list', () => {
    const to = params('type=views');
    copyRumVizParams(params('m=count&by=a'), to);

    expect(to.toString()).toBe('type=views');
  });
});
