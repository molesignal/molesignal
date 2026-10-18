import i18next from 'i18next';
import { beforeAll, describe, expect, it } from 'vitest';

import en from '@/i18n/en-us/agent.json';
import zh from '@/i18n/zh-cn/agent.json';

import { activityKind, activitySummary } from './activity';

beforeAll(async () => {
  await i18next.init({
    lng: 'en',
    resources: { en: { agent: en }, zh: { agent: zh } },
    interpolation: { escapeValue: false },
  });
});

describe('an answer’s work in one line', () => {
  it('sorts tools by what they do', () => {
    expect(activityKind('list_streams')).toBe('look');
    expect(activityKind('query_logs')).toBe('query');
    expect(activityKind('read_knowledge')).toBe('knowledge');
    expect(activityKind('edit_knowledge')).toBe('knowledge_edit');
    expect(activityKind('propose_dashboard_creation')).toBe('change');
    expect(activityKind('ask_user')).toBe('question');
    expect(activityKind('mystery')).toBe('tool');
  });

  it('counts each kind, in the order it first came, in the reader’s language', () => {
    const tools = ['list_knowledge', 'list_teams', 'list_streams', 'query_logs', 'list_alert_rules'];
    const english = i18next.getFixedT('en', 'agent');
    expect(activitySummary(tools, english)).toBe(
      'Read the knowledge base, looked at 3 items, ran 1 query',
    );
    const chinese = i18next.getFixedT('zh', 'agent');
    expect(activitySummary(tools, chinese)).toBe('读了 1 次知识库，查看了 3 项，查询了 1 次');
  });
});
