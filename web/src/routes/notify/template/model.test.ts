import { describe, expect, it } from 'vitest';

import {
  DEFAULT_ONCALL_TEMPLATE_PREVIEW,
  DEFAULT_NOTIFY_TEMPLATE_PREVIEW,
  defaultNotifyTemplatePreview,
  renderNotifyTemplate,
} from './model';

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

  it('keeps missing placeholders visible', () => {
    expect(
      renderNotifyTemplate(
        '{{labels.missing}} / {{unknown.path}}',
        DEFAULT_NOTIFY_TEMPLATE_PREVIEW,
      ),
    ).toBe('{{labels.missing}} / {{unknown.path}}');
  });
});
