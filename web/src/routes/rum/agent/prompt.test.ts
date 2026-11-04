import { beforeAll, describe, expect, it } from 'vitest';

import i18n from '@/i18n';

import { type RumAgentSubject, type RumFocus, rumPrompt, rumReference } from './prompt';

const subject = (focus?: RumFocus, environment = 'prod'): RumAgentSubject => ({
  application: { id: 'shop-web', name: 'Shop' },
  environment,
  range: { from: 1_000_000_000_000, to: 1_003_600_000_000 },
  ...(focus && { focus }),
});

const stamp = (micros: number) => `T${micros / 1_000_000}`;

const FOCUSES: Array<[string, RumFocus | undefined]> = [
  ['the application', undefined],
  ['the errors', { kind: 'errors' }],
  ['the performance', { kind: 'performance' }],
  ['the frustration', { kind: 'frustration' }],
  ['a vital', { kind: 'vital', vital: 'inp' }],
  ['an issue', { kind: 'issue', id: '3f9c', type: 'TypeError', message: 'x is undefined' }],
  ['a page', { kind: 'slice', dimension: 'Page', value: '/checkout' }],
  ['a page with its figure', { kind: 'slice', dimension: 'Page', value: '/checkout', vital: 'lcp', figure: '4.3 s' }],
  ['an element', { kind: 'element', name: 'Buy now', selector: null }],
  ['an element with its selector', { kind: 'element', name: 'Buy now', selector: 'button#buy' }],
  ['a session', { kind: 'session', id: '6f1c' }],
  ['an alert', { kind: 'incident', summary: 'LCP is slow', sessions: [] }],
  ['an alert with its sessions', { kind: 'incident', summary: 'LCP is slow', sessions: ['s-1', 's-2'] }],
];

describe.each(['en-us', 'zh-cn'])('the question a chat started from RUM opens with (%s)', (language) => {
  beforeAll(async () => {
    await i18n.changeLanguage(language);
  });

  it.each(FOCUSES)('names the application, the environment and the range, about %s', (_, focus) => {
    const t = i18n.getFixedT(language, 'rum');
    const prompt = rumPrompt(t, stamp, subject(focus));

    expect(prompt).toContain('Shop');
    expect(prompt).toContain('prod');
    expect(prompt).toContain('T1000000');
    expect(prompt).toContain('T1003600');
    // Every place in the sentence is filled.
    expect(prompt).not.toMatch(/{{|}}/);
  });

  it('says every environment when none is chosen', () => {
    const t = i18n.getFixedT(language, 'rum');

    expect(rumPrompt(t, stamp, subject(undefined, ''))).toContain(t('agent.all_environments'));
  });
});

describe('the focus of a question', () => {
  beforeAll(async () => {
    await i18n.changeLanguage('en-us');
  });

  const ask = (focus?: RumFocus) => rumPrompt(i18n.getFixedT('en-us', 'rum'), stamp, subject(focus));

  it('is the vital in capitals', () => {
    expect(ask({ kind: 'vital', vital: 'inp' })).toContain('INP');
  });

  it('is an issue by what it threw and by its id, with a long message cut', () => {
    const issue = ask({ kind: 'issue', id: '3f9c', type: 'TypeError', message: 'm'.repeat(400) });

    expect(issue).toContain('TypeError: mmm');
    expect(issue).toContain('3f9c');
    expect(issue).toContain('…');
    expect(issue.length).toBeLessThan(700);
  });

  it('is a value of a dimension, with the figure of the vital when the row had one', () => {
    expect(ask({ kind: 'slice', dimension: 'Page', value: '/checkout' })).toContain('Page is /checkout');
    expect(ask({ kind: 'slice', dimension: 'Page', value: '/checkout', vital: 'lcp', figure: '4.3 s' })).toContain(
      'LCP 75th percentile is 4.3 s',
    );
  });

  it('is an element, with its selector when it has one', () => {
    expect(ask({ kind: 'element', name: 'Buy now', selector: null })).not.toContain('button#buy');
    expect(ask({ kind: 'element', name: 'Buy now', selector: 'button#buy' })).toContain('(button#buy)');
  });

  it('is an alert, with the sessions it was told by when it has them', () => {
    expect(ask({ kind: 'incident', summary: 'LCP is slow', sessions: [] })).not.toContain('sessions:');
    expect(ask({ kind: 'incident', summary: 'LCP is slow', sessions: ['s-1', 's-2'] })).toContain('s-1, s-2');
  });
});

describe('the reference a question carries', () => {
  it('is the application', () => {
    expect(rumReference(subject())).toEqual({ kind: 'rum_application', id: 'shop-web', label: 'Shop' });
  });
});
