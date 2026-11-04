import { describe, expect, it } from 'vitest';

import type { RumRow } from '@/api/rum';
import type { Incident, TriggeringSample } from '@/types/alerting';

import { incidentFigure, readSession, sampleSessionPath, sessionSamples, sessionsQuery } from './samples';

const SECOND = 1_000_000;

function sample(session: string | null, patch: Record<string, string> = {}, ts = 1_790_000_000 * SECOND): TriggeringSample {
  return {
    ts,
    value: 4.3,
    labels: { value: '4.3', application: 'storefront', ...(session ? { 'rum.session_id': session } : {}), ...patch },
  };
}

function incident(samples: TriggeringSample[]): Pick<Incident, 'triggeringQuery' | 'labels'> {
  return { labels: {}, triggeringQuery: { language: 'sql', statement: 'rum:lcp_p75 storefront', sampleValues: samples } };
}

describe('the figure of an incident', () => {
  it('is the figure of its rule while the rule is known', () => {
    const figure = incidentFigure(
      { labels: { 'rum.application': 'other' } },
      {
        query: {
          language: 'sql',
          statement: '',
          periodSecs: 3_600,
          rum: { applicationId: 'storefront', environment: 'prod', page: '/checkout', metric: 'lcp_p75' },
        },
      },
    );

    expect(figure).toEqual({ application: 'storefront', environment: 'prod', page: '/checkout', version: null, metric: 'lcp_p75' });
  });

  it('is what the labels of the rule editor say when the rule is not there, without the figure', () => {
    const figure = incidentFigure(
      { labels: { 'rum.application': 'storefront', 'deployment.environment': 'prod', 'rum.version': '2.4.1' } },
      undefined,
    );

    expect(figure).toEqual({ application: 'storefront', environment: 'prod', page: null, version: '2.4.1', metric: null });
  });

  it('is none for an incident of any other rule', () => {
    expect(incidentFigure({ labels: { service: 'checkout' } }, undefined)).toBeNull();
    expect(
      incidentFigure(
        { labels: {} },
        { query: { language: 'sql', statement: 'SELECT 1', periodSecs: 60, stream: { name: 'app', streamType: 'logs' } } },
      ),
    ).toBeNull();
  });
});

describe('the sessions of an incident', () => {
  it('are the rows of its sample that name one, each once, with the figure and what went with it', () => {
    const found = sessionSamples(
      incident([
        sample('s-1', { 'rum.value': '7213.5', 'rum.detail': 'img.hero' }, 10 * SECOND),
        sample(null),
        sample('s-1', { 'rum.value': '1' }, 11 * SECOND),
        sample('s-2', {}, 12 * SECOND),
        sample('s-3', { 'rum.value': 'not a number', 'rum.detail': '  ' }, 13 * SECOND),
      ]),
    );

    expect(found).toEqual([
      { session: 's-1', at: 10 * SECOND, value: 7213.5, detail: 'img.hero' },
      { session: 's-2', at: 12 * SECOND, value: null, detail: null },
      { session: 's-3', at: 13 * SECOND, value: null, detail: null },
    ]);
  });

  it('are at most three, and none for an incident that kept no sample', () => {
    const many = Array.from({ length: 6 }, (_, index) => sample(`s-${index}`));

    expect(sessionSamples(incident(many))).toHaveLength(3);
    expect(sessionSamples({ triggeringQuery: null })).toEqual([]);
    expect(sessionSamples(incident([sample(null)]))).toEqual([]);
  });

  it('are read by their ids, from a day before the first of them to just after the last', () => {
    const query = sessionsQuery([
      { session: 's-1', at: 100 * SECOND, value: null, detail: null },
      { session: 's-2', at: 50 * SECOND, value: null, detail: null },
    ]);

    expect(query).toMatchObject({
      type: 'sessions',
      from: 50 * SECOND - 86_400 * SECOND,
      to: 101 * SECOND,
      filters: [{ field: 'session.id', op: 'in', value: '["s-1","s-2"]' }],
      limit: 2,
    });
    expect(query.columns).toContain('session.has_replay');
  });
});

describe('a session read', () => {
  const row = (fields: Record<string, unknown>): RumRow => ({ timestamp: 5 * SECOND, ...fields });

  it('says who it was by a name, then an address, then an id, and a visitor by the id the SDK keeps', () => {
    expect(readSession(row({ 'usr.name': 'Ada', 'usr.email': 'ada@example.com', 'usr.id': 'u-1' })).user).toBe('Ada');
    expect(readSession(row({ 'usr.email': 'ada@example.com', 'usr.id': 'u-1' })).user).toBe('ada@example.com');
    expect(readSession(row({ 'usr.id': 'u-1' }))).toMatchObject({ user: 'u-1', anonymous: false });
    expect(readSession(row({ 'usr.anonymous_id': 'anon-7c2e' }))).toMatchObject({ user: 'anon-7c2e', anonymous: true });
    expect(readSession(row({})).user).toBeNull();
  });

  it('says in what, and whether it was recorded, and when it began', () => {
    expect(readSession(row({ 'browser.name': 'Chrome', 'browser.version': '151', 'os.name': 'macOS', 'session.has_replay': true }))).toEqual({
      user: null,
      anonymous: false,
      browser: 'Chrome 151',
      system: 'macOS',
      replay: true,
      began: 5 * SECOND,
    });
  });

  it('is nothing known for a session that was not found', () => {
    expect(readSession(undefined)).toEqual({ user: null, anonymous: false, browser: null, system: null, replay: false, began: null });
  });
});

describe('where a sample session opens', () => {
  const sampleOf = { session: 'abc', at: 100 * SECOND, value: null, detail: null };

  it('is its replay five seconds before the moment, and not before the session began', () => {
    const early = sampleSessionPath(sampleOf, { user: null, anonymous: false, browser: null, system: null, replay: true, began: 60 * SECOND });
    const late = sampleSessionPath(sampleOf, { user: null, anonymous: false, browser: null, system: null, replay: true, began: 99 * SECOND });

    expect(new URL(early.to, 'https://x.test').searchParams.get('at')).toBe(String(95 * SECOND));
    expect(early.offset).toBe(35_000);
    expect(new URL(late.to, 'https://x.test').searchParams.get('at')).toBe(String(99 * SECOND));
    expect(late.offset).toBe(0);
    expect(early.to.startsWith('/rum/sessions/view/abc?')).toBe(true);
  });

  it('is the session alone when it was not recorded, or is not known', () => {
    const plain = sampleSessionPath(sampleOf, { user: null, anonymous: false, browser: null, system: null, replay: false, began: 60 * SECOND });
    const unknown = sampleSessionPath(sampleOf, readSession(undefined));

    for (const opened of [plain, unknown]) {
      expect(new URL(opened.to, 'https://x.test').searchParams.has('at')).toBe(false);
      expect(opened.offset).toBeNull();
    }
  });
});
