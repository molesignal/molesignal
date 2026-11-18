import { describe, expect, it } from 'vitest';

import { guideDashboardLink, guideIsEmpty, guideQueryLink } from './guideModel';
import type { IncidentTimeWindow } from './InvestigationActions';

const time: IncidentTimeWindow = {
  from: '2026-08-14T05:00:00.000Z',
  to: '2026-08-14T06:00:00.000Z',
};

function open(to: string): URL {
  return new URL(to, 'https://molesignal.test');
}

describe('incident guide', () => {
  it('says whether a rule has anything in its guide', () => {
    expect(guideIsEmpty(undefined)).toBe(true);
    expect(guideIsEmpty({})).toBe(true);
    expect(guideIsEmpty({ steps: ' \n ' })).toBe(true);
    expect(guideIsEmpty({ steps: 'Check the gateway.' })).toBe(false);
    expect(guideIsEmpty({ dashboards: ['dash-1'] })).toBe(false);
  });

  it('opens a PromQL query in Metrics over the incident window', () => {
    const link = open(guideQueryLink({ name: 'Rate', language: 'promql', statement: ' up ' }, time));

    expect(link.pathname).toBe('/metrics');
    expect(link.searchParams.get('promql')).toBe('up');
    expect(link.searchParams.get('time')).toBe(`${time.from}..${time.to}`);
    expect(link.searchParams.get('stream')).toBeNull();
  });

  it('opens a SQL query in Logs or, on a trace stream, in Traces with its stream', () => {
    const logs = open(
      guideQueryLink(
        { name: 'Errors', language: 'sql', stream: { name: 'app_logs', streamType: 'logs' }, statement: 'SELECT 1' },
        time,
      ),
    );

    expect(logs.pathname).toBe('/logs');
    expect(logs.searchParams.get('sql')).toBe('SELECT 1');
    expect(logs.searchParams.get('stream')).toBe('app_logs');
    expect(logs.searchParams.get('streamType')).toBe('logs');

    const traces = open(
      guideQueryLink(
        { name: 'Slow', language: 'sql', stream: { name: 'traces', streamType: 'traces' }, statement: 'SELECT 2' },
        time,
      ),
    );

    expect(traces.pathname).toBe('/traces');
    expect(traces.searchParams.get('streamType')).toBe('traces');
  });

  it('opens a dashboard by its id', () => {
    expect(guideDashboardLink('dash 1')).toBe('/dashboards/dash%201');
  });
});
