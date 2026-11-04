import { describe, expect, it } from 'vitest';

import type { IntakeAlert, IntakeRequest } from '@/api/alertSources';
import type { Incident } from '@/types/alerting';

import {
  TOKEN_PLACEHOLDER,
  activeIncidentsOf,
  dayFigures,
  distinctLabels,
  intakeAddress,
  labelsFromPairs,
  outcomeCounts,
  setupSnippet,
  sourceState,
} from './model';

const HOUR = 3_600_000_000;
const NOW = 1_800_000_000_000_000;

function request(hoursAgo: number, status: IntakeRequest['status'], outcomes: IntakeAlert['outcome'][] = []): IntakeRequest {
  return {
    id: `r-${hoursAgo}-${status}`,
    receivedAt: NOW - hoursAgo * HOUR,
    viaCluster: '',
    status,
    httpStatus: status === 'rejected' ? 401 : 200,
    message: status === 'rejected' ? 'invalid alert source token' : null,
    alertCount: outcomes.length,
    alerts: outcomes.map((outcome) => ({ action: 'trigger', dedupKey: 'k', summary: 's', severity: null, outcome })),
    bodyExcerpt: null,
  };
}

const source = { kind: 'alertmanager' as const, intakePath: '/api/v1/intake/alerts' };

describe('alert source model', () => {
  it('builds the address, with the token in it only for a sender that cannot set headers', () => {
    expect(intakeAddress('https://obs.example.com/', source)).toBe('https://obs.example.com/api/v1/intake/alerts');
    expect(intakeAddress('https://obs.example.com', { ...source, kind: 'aliyun_cms' })).toBe(
      `https://obs.example.com/api/v1/intake/alerts?token=${TOKEN_PLACEHOLDER}`,
    );
    expect(intakeAddress('https://obs.example.com', { ...source, kind: 'aliyun_cms' }, 'mspat-ab')).toBe(
      'https://obs.example.com/api/v1/intake/alerts?token=mspat-ab',
    );
    for (const kind of ['tencent_monitor', 'aws_cloudwatch'] as const) {
      expect(intakeAddress('https://obs.example.com', { ...source, kind }, 'mspat-ab')).toBe(
        'https://obs.example.com/api/v1/intake/alerts?token=mspat-ab',
      );
    }
  });

  it('writes each sender its own setup, with the token when it is known', () => {
    const url = 'https://obs.example.com/api/v1/intake/alerts';

    expect(setupSnippet('alertmanager', url)?.content).toContain(`credentials: ${TOKEN_PLACEHOLDER}`);
    expect(setupSnippet('alertmanager', url, 'mspat-xy')?.content).toContain('credentials: mspat-xy');
    expect(setupSnippet('grafana', url)?.content).toContain('authorization_scheme: Bearer');
    expect(setupSnippet('datadog', url)?.content).toContain('"transition": "$ALERT_TRANSITION"');
    expect(setupSnippet('pagerduty', url)?.content).toContain('incident.resolved');
    expect(setupSnippet('api', url)?.language).toBe('bash');
    // Aliyun is set up by its address alone, the token in it.
    expect(setupSnippet('aliyun_cms', url)).toBeNull();
    expect(setupSnippet('tencent_monitor', url)).toBeNull();
    // CloudWatch reaches it through an SNS topic, which subscribes the address.
    expect(setupSnippet('aws_cloudwatch', `${url}?token=mspat-xy`)?.content).toContain(
      `--notification-endpoint '${url}?token=mspat-xy'`,
    );
  });

  it('gives Zabbix a media type to import, with its macros under our field names', () => {
    const url = 'https://obs.example.com/api/v1/intake/alerts';
    const snippet = setupSnippet('zabbix', url, 'mspat-xy');

    expect(snippet?.language).toBe('yaml');
    expect(snippet?.content).toContain(`value: '${url}'`);
    expect(snippet?.content).toContain("value: 'mspat-xy'");
    expect(snippet?.content).toContain("- name: event_id\n          value: '{EVENT.ID}'");
    expect(snippet?.content).toContain("- name: zabbix_url\n          value: '{$ZABBIX.URL}'");
    // Problem, recovery and update operations all need a message of their own.
    for (const mode of ['PROBLEM', 'RECOVERY', 'UPDATE']) {
      expect(snippet?.content).toContain(`operation_mode: ${mode}`);
    }
    expect(setupSnippet('zabbix', url)?.content).toContain(`value: '${TOKEN_PLACEHOLDER}'`);
  });

  it('has a mail system post the raw message', () => {
    const snippet = setupSnippet('email', 'https://obs.example.com/api/v1/intake/alerts', 'mspat-xy');

    expect(snippet?.content).toContain("Content-Type: message/rfc822");
    expect(snippet?.content).toContain('Bearer mspat-xy');
    expect(snippet?.content).toContain('--data-binary @alert.eml');
  });

  it('counts the outcomes of a request, the most consequential first', () => {
    const alert = (outcome: IntakeAlert['outcome']): IntakeAlert => ({
      action: 'trigger',
      dedupKey: 'k',
      summary: 's',
      severity: null,
      outcome,
    });

    expect(outcomeCounts([alert('unchanged'), alert('opened'), alert('unchanged')])).toEqual([
      { outcome: 'opened', count: 1 },
      { outcome: 'unchanged', count: 2 },
    ]);
  });

  it('keeps named labels and finds what tells a group apart', () => {
    expect(labelsFromPairs([{ key: ' env ', value: ' prod ' }, { key: '', value: 'x' }])).toEqual({ env: 'prod' });
    expect(
      distinctLabels([
        { labels: { alertname: 'HighCPU', instance: 'web-1' } },
        { labels: { alertname: 'HighCPU', instance: 'web-2' } },
      ]),
    ).toEqual(['instance']);
    expect(distinctLabels([{ labels: { a: '1' } }])).toEqual([]);
  });

  it('tells how a source stands from its latest request', () => {
    const on = { enabled: true, lastIntakeAt: null };

    expect(sourceState(on, []).state).toBe('waiting');
    expect(sourceState({ ...on, lastIntakeAt: NOW - 30 * 24 * HOUR }, []).state).toBe('quiet');
    expect(sourceState(on, [request(1, 'rejected'), request(2, 'accepted')]).state).toBe('refused');
    expect(sourceState(on, [request(1, 'forwarded'), request(2, 'rejected')]).state).toBe('receiving');
    expect(sourceState({ ...on, enabled: false }, [request(1, 'accepted')]).state).toBe('off');
  });

  it('counts the last day by the hour, and says when the page may not hold it all', () => {
    const requests = [
      request(0, 'accepted', ['opened']),
      request(0.5, 'rejected'),
      request(5, 'accepted', ['resolved', 'unchanged']),
      request(30, 'accepted', ['opened']),
    ];
    const figures = dayFigures(requests, 200, NOW);

    expect(figures).toMatchObject({ requests: 3, capped: false, refused: 1, alerts: 3, opened: 1, resolved: 1 });
    expect(figures.hourly).toHaveLength(24);
    expect(figures.hourly[23]).toBe(2);
    expect(figures.hourly[18]).toBe(1);
    expect(dayFigures(requests.slice(0, 3), 3, NOW).capped).toBe(true);
  });

  it('finds the incidents a source opened that are still open', () => {
    const incident = (id: string, status: Incident['status'], source: string, createdAt: number) =>
      ({
        id,
        status,
        ruleId: source,
        createdAt: createdAt,
        labels: { 'alert.source': 'external', 'external.kind': 'alertmanager' },
      }) as unknown as Incident;
    const ruleIncident = { ...incident('rule', 'open', 'src-1', 1), labels: {} } as Incident;

    expect(
      activeIncidentsOf('src-1', [
        incident('late', 'acknowledged', 'src-1', 20),
        incident('early', 'open', 'src-1', 10),
        incident('done', 'resolved', 'src-1', 5),
        incident('other', 'open', 'src-2', 1),
        ruleIncident,
      ]).map((item) => item.id),
    ).toEqual(['early', 'late']);
  });
});
