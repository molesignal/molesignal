import { describe, expect, it } from 'vitest';

import { applyChange, readView } from './address';
import { CLOSE_VIEW, openViewChange } from './openView';

const params = (query: string) => new URLSearchParams(query);

describe('what an address says of the performance page', () => {
  it('is the first level, the pages, the most views and no drawer when it says nothing', () => {
    expect(readView(params(''))).toEqual({
      metric: 'lcp',
      by: 'page',
      sort: 'views',
      direction: undefined,
      search: '',
      open: '',
      tab: 'lcp',
      drawer: null,
    });
  });

  it('says the level in front, what the table is of, how it is ordered, the words and what is open', () => {
    expect(readView(params('vital=inp&by=browser&sort=fcp&dir=asc&q=chrome&open=Chrome&tab=cls'))).toEqual({
      metric: 'inp',
      by: 'browser',
      sort: 'fcp',
      direction: 'asc',
      search: 'chrome',
      open: 'Chrome',
      tab: 'cls',
      drawer: null,
    });
  });

  it('says the view whose load is open over the drawer, and what it was opened on', () => {
    expect(readView(params('open=%2Fcheckout&view=v-1&at=1760000000000000&focus=r-9')).drawer).toEqual({ id: 'v-1', at: 1_760_000_000_000_000, focus: 'r-9' });
    // Without the moment that led there the view is looked for over the last month, and without a request none is chosen.
    expect(readView(params('view=v-1')).drawer).toEqual({ id: 'v-1', at: null, focus: '' });
    expect(readView(params('view=v-1&at=nonsense')).drawer).toEqual({ id: 'v-1', at: null, focus: '' });
  });

  it('opens the drawer on the vital in front, or on the first when the one in front is not a vital', () => {
    expect(readView(params('vital=inp')).tab).toBe('inp');
    expect(readView(params('vital=ttfb')).tab).toBe('lcp');
    expect(readView(params('vital=inp&tab=nonsense')).tab).toBe('inp');
  });

  it('takes what it does not know for what the page opens on', () => {
    expect(readView(params('vital=nonsense&by=nonsense&sort=nonsense&dir=sideways'))).toMatchObject({
      metric: 'lcp',
      by: 'page',
      sort: 'views',
      direction: undefined,
    });
  });
});

describe('the address after a change', () => {
  it('sets what is changed and leaves the rest', () => {
    expect(applyChange(params('time=1&vital=inp'), { by: 'browser' }).toString()).toBe('time=1&vital=inp&by=browser');
  });

  it('leaves out what the page opens on, so that an address that was not touched stays as it is', () => {
    expect(applyChange(params('vital=inp'), { vital: 'lcp' }).toString()).toBe('');
    expect(applyChange(params('by=browser'), { by: 'page' }).toString()).toBe('');
    expect(applyChange(params('sort=lcp'), { sort: 'views' }).toString()).toBe('');
  });

  it('takes out what is given none', () => {
    expect(applyChange(params('q=chrome&dir=asc'), { q: null, dir: null }).toString()).toBe('');
  });

  it('lets go of what is open when the dimension changes, since it was chosen in the one that is left', () => {
    expect(applyChange(params('by=browser&open=Chrome&tab=inp'), { by: 'os' }).toString()).toBe('by=os');
    // A change that names both is not undone.
    expect(applyChange(params('by=browser&open=Chrome'), { by: 'os', open: 'macOS' }).toString()).toBe('by=os&open=macOS');
  });

  it('opens a view over the drawer and lets go of it again, leaving what stands under it', () => {
    const opened = applyChange(params('open=%2Fcheckout'), openViewChange('v-1', 1_760_000_000_000_000.25, ''));

    expect(opened.toString()).toBe('open=%2Fcheckout&view=v-1&at=1760000000000000');
    expect(applyChange(opened, CLOSE_VIEW).toString()).toBe('open=%2Fcheckout');
  });

  it('does not touch the time range the page keeps in its address', () => {
    expect(applyChange(params('time=24h&app=storefront'), { vital: 'cls' }).toString()).toBe('time=24h&app=storefront&vital=cls');
  });
});
