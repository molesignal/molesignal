import { describe, expect, it } from 'vitest';

import {
  cardColor,
  cardProblem,
  cardToSave,
  characterCount,
  DEFAULT_ONCALL_TEMPLATE_PREVIEW,
  DEFAULT_NOTIFY_TEMPLATE_PREVIEW,
  defaultNotifyTemplatePreview,
  durationText,
  notifyTemplatePreview,
  renderCard,
  renderNotifyTemplate,
  startingCard,
} from './model';
import {
  providerDraft,
  providerDrafts,
  draftsProblem,
  type ProviderDrafts,
  providerProblem,
  providerTemplates,
} from './providerDrafts';

describe('notify template model', () => {
  it('renders alert placeholders and common Notify paths', () => {
    expect(
      renderNotifyTemplate(
        '{{rule.name}} {{incident.summary}} {{evaluated_at}} {{labels.service}} {{event.id}}',
        DEFAULT_NOTIFY_TEMPLATE_PREVIEW,
      ),
    ).toBe(
      'API unavailable API unavailable 1785283200000000 api alert.triggered:preview',
    );

    expect(
      renderNotifyTemplate(
        '{{ rule.id }} {{incident.id}} {{event.attributes.rule_name}}',
        DEFAULT_NOTIFY_TEMPLATE_PREVIEW,
      ),
    ).toBe('rule-preview incident-preview API unavailable');
  });

  it('renders on-call schedule, shift, and override placeholders', () => {
    expect(
      renderNotifyTemplate(
        '{{schedule.name}} {{oncall.current_user_id}} -> {{oncall.next_user_id}} @ {{oncall.transition_at}} {{override.reason}}',
        DEFAULT_ONCALL_TEMPLATE_PREVIEW,
      ),
    ).toBe(
      'Primary alice -> bob @ 1785285000000000 Temporary coverage',
    );
  });

  it('uses alert context when previewing an escalation template', () => {
    const input = defaultNotifyTemplatePreview('escalation');

    expect(input.eventType).toBe('alert.escalated');
    expect(
      renderNotifyTemplate(
        '{{rule.name}} {{incident.id}} {{labels.env}} {{value}} {{threshold}}',
        input,
      ),
    ).toBe('API unavailable incident-preview production 512 500');
  });

  it.each([
    [
      'report',
      '{{event.attributes.report_name}}',
      'Weekly reliability report',
    ],
    [
      'security',
      '{{event.attributes.actor}}',
      'alice@example.com',
    ],
    ['system', '{{event.attributes.component}}', 'query-gateway'],
  ] as const)(
    'provides concrete %s preview attributes',
    (category, body, expected) => {
      expect(
        renderNotifyTemplate(
          body,
          defaultNotifyTemplatePreview(category),
        ),
      ).toBe(expected);
    },
  );

  it('writes times, durations, lists and labels for people, as the server does', () => {
    expect(
      renderNotifyTemplate(
        '{{event.time}} | {{incident.started_time}} | {{incident.duration}} | {{affected_services}} | {{incident.assignees}} | {{labels}}',
        DEFAULT_NOTIFY_TEMPLATE_PREVIEW,
      ),
    ).toBe(
      '2026-07-29 00:00:00 UTC | 2026-07-28 23:55:00 UTC | 5m | api, checkout | alice | env=production, environment=production, service=api',
    );
    expect(
      renderNotifyTemplate(
        '{{incident.url}} {{rule.description}}',
        DEFAULT_NOTIFY_TEMPLATE_PREVIEW,
      ),
    ).toBe(
      'https://molesignal.example.com/alerts/incidents/incident-preview The API answers fewer than 99% of requests.',
    );
    expect(
      renderNotifyTemplate(
        '{{escalation.step}} {{escalation.loop}} {{escalation.target}}',
        defaultNotifyTemplatePreview('escalation'),
      ),
    ).toBe('2 1 schedule');
    expect(
      renderNotifyTemplate(
        '{{oncall.transition_time}} | {{override.start_time}} | {{override.end_time}}',
        DEFAULT_ONCALL_TEMPLATE_PREVIEW,
      ),
    ).toBe('2026-07-29 00:30:00 UTC | 2026-07-29 00:00:00 UTC | 2026-07-29 02:00:00 UTC');
  });

  it('previews a preset on the event it is written for', () => {
    const resolved = notifyTemplatePreview('alert', 'alert.resolved');
    expect(resolved.eventType).toBe('alert.resolved');
    expect(
      renderNotifyTemplate(
        '{{incident.status}} {{incident.acknowledged_by}} | {{incident.resolved_time}} | {{incident.duration}} | {{event.time}}',
        resolved,
      ),
    ).toBe('resolved alice | 2026-07-29 01:00:00 UTC | 1h 5m | 2026-07-29 01:00:00 UTC');

    const acknowledged = notifyTemplatePreview('alert', 'alert.acknowledged');
    expect(
      renderNotifyTemplate('{{incident.acknowledged_time}} {{incident.resolved_time}}', acknowledged),
    ).toBe('2026-07-29 00:10:00 UTC {{incident.resolved_time}}');

    expect(notifyTemplatePreview('alert', 'alert.triggered')).toBe(DEFAULT_NOTIFY_TEMPLATE_PREVIEW);
    expect(notifyTemplatePreview('oncall', 'oncall.override.created')).toBe(
      DEFAULT_ONCALL_TEMPLATE_PREVIEW,
    );
  });

  it('writes a duration as its two largest units', () => {
    const seconds = (value: number) => durationText(value * 1_000_000);
    expect(durationText(400_000)).toBe('0s');
    expect(seconds(59)).toBe('59s');
    expect(seconds(260)).toBe('4m 20s');
    expect(seconds(3_600)).toBe('1h');
    expect(seconds(3_900)).toBe('1h 5m');
    expect(seconds(90_061)).toBe('1d 1h');
    expect(seconds(-5)).toBe('0s');
  });

  it('keeps missing placeholders visible', () => {
    expect(
      renderNotifyTemplate(
        '{{labels.missing}} / {{unknown.path}}',
        DEFAULT_NOTIFY_TEMPLATE_PREVIEW,
      ),
    ).toBe('{{labels.missing}} / {{unknown.path}}');
  });

  it('saves an Alibaba Cloud template only when it has a code, checked as the server checks it', () => {
    const empty = providerDraft(undefined);
    expect(providerProblem(empty)).toBeNull();
    expect(providerTemplates(drafts({ aliyun: empty }))).toEqual({});

    const draft = {
      code: ' SMS_1 ',
      params: [
        { name: 'name', value: '{{rule.name}}' },
        { name: '', value: '' },
      ],
      maxChars: '20',
    };
    expect(providerProblem(draft)).toBeNull();
    expect(providerTemplates(drafts({ aliyun: draft }))).toEqual({
      aliyun: { code: 'SMS_1', params: { name: '{{rule.name}}' }, maxParamChars: 20 },
    });

    expect(providerProblem({ ...draft, code: '' })).toBe('code');
    expect(providerProblem({ ...draft, params: [{ name: '${name}', value: 'x' }] })).toBe('param_name');
    expect(providerProblem({ ...draft, params: [{ name: 'name', value: ' ' }] })).toBe('param_value');
    expect(
      providerProblem({
        ...draft,
        params: [
          { name: 'name', value: 'a' },
          { name: 'name', value: 'b' },
        ],
      }),
    ).toBe('param_twice');
    expect(providerProblem({ ...draft, maxChars: '501' })).toBe('max_chars');

    // A saved template comes back as it was written.
    expect(providerDraft({ code: 'TTS_1', params: { content: '{{rule.name}}' } })).toEqual({
      code: 'TTS_1',
      params: [{ name: 'content', value: '{{rule.name}}' }],
      maxChars: '',
    });
  });

  it('counts characters as a phone does', () => {
    expect(characterCount('告警 😀')).toBe(4);
  });

  it('fills a card the way the server does and leaves out what an event cannot fill', () => {
    const card = {
      title: '[{{severity}}] {{rule.name}}',
      color: 'severity' as const,
      fields: [
        { label: 'Service', value: '{{labels.service}}' },
        { label: 'Host', value: '{{labels.host}}' },
        { label: '', value: '' },
      ],
      links: [
        { label: 'Runbook', value: '', url: '{{annotations.runbook_url}}' },
        { label: 'Mail', url: 'mailto:{{labels.service}}' },
      ].map(({ label, url }) => ({ label, url })),
      note: ' MoleSignal ',
    };

    expect(renderCard(card, DEFAULT_NOTIFY_TEMPLATE_PREVIEW)).toEqual({
      title: '[critical] API unavailable',
      color: 'severity',
      fields: [{ label: 'Service', value: 'api' }],
      links: [{ label: 'Runbook', url: 'https://runbooks.example.com/api-unavailable' }],
      note: 'MoleSignal',
    });
    expect(cardToSave(card).fields).toHaveLength(2);
    expect(cardColor('severity', DEFAULT_NOTIFY_TEMPLATE_PREVIEW.attributes)).toBe('red');
    expect(cardColor('severity', { severity: 'critical', status: 'resolved' })).toBe('green');
    expect(cardColor('grey', {})).toBe('grey');
  });

  it('checks a card as the server checks it', () => {
    // The last part of a key is as long as a label gets.
    const t = ((key: string) => key.split('.').pop()) as never;
    const card = startingCard('alert', t);

    expect(card.fields.map((field) => field.value)).toEqual([
      '{{severity}}',
      '{{incident.status}}',
      '{{labels.service}}',
    ]);
    expect(cardProblem(card)).toBeNull();
    expect(cardProblem({ ...card, fields: [{ label: 'Service', value: '' }] })).toBe('field');
    expect(cardProblem({ ...card, links: [{ label: 'Mail', url: 'mailto:a@b.c' }] })).toBe('link_url');
    expect(cardProblem({ ...card, title: 'x'.repeat(201) })).toBe('too_long');
    expect(
      cardProblem({ ...card, fields: Array.from({ length: 11 }, () => ({ label: 'a', value: 'b' })) }),
    ).toBe('too_many');
    expect(startingCard('report', t).fields).toEqual([]);
  });
});

/** The drafts of a template with only these providers written. */
function drafts(written: Partial<ProviderDrafts>): ProviderDrafts {
  return { ...providerDrafts(undefined), ...written };
}

describe('templates of several providers', () => {
  it('saves each provider that has a code, and keeps the others out', () => {
    const saved = providerTemplates(
      drafts({
        tencent: { code: '1110', params: [{ name: '1', value: '{{rule.name}}' }], maxChars: '12' },
        huawei: { code: 'tmpl', params: [{ name: '1', value: '{{severity}}' }], maxChars: '' },
      }),
    );

    expect(saved).toEqual({
      tencent: { code: '1110', params: { '1': '{{rule.name}}' }, maxParamChars: 12 },
      huawei: { code: 'tmpl', params: { '1': '{{severity}}' } },
    });
  });

  it('reads back what was saved per provider', () => {
    const read = providerDrafts({ tencent: { code: '1110', params: { '1': 'x' } } });

    expect(read.tencent.code).toBe('1110');
    expect(read.aliyun.code).toBe('');
    expect(read.huawei.code).toBe('');
  });

  it('names the provider that stops the save', () => {
    expect(draftsProblem(drafts({}))).toBeNull();
    expect(
      draftsProblem(drafts({ huawei: { code: '', params: [{ name: '1', value: 'x' }], maxChars: '' } })),
    ).toEqual({ provider: 'huawei', problem: 'code' });
  });
});
