import { describe, expect, it } from 'vitest';

import type { ChatMessageRow } from '@/api/agent/chat';

import { chatMarkdown, exportFileName } from './exportChat';

const LABELS = {
  you: 'You',
  agent: 'Mole Agent',
  conclusion: 'Conclusion',
  evidence: 'Evidence',
  likelyCauses: 'Likely causes',
  limitations: 'Limitations',
  nextSteps: 'Next steps',
};

function row(id: string, role: string, content: string, minute: number): ChatMessageRow {
  return {
    id,
    chat_id: 'chat-1',
    org_id: 'org-1',
    role,
    content,
    created_at_micros: Date.UTC(2026, 4, 23, 9, minute) * 1000,
  };
}

describe('/export', () => {
  it('writes the chat as Markdown, answers by their sections', () => {
    const markdown = chatMarkdown(
      'Checkout latency',
      [
        row('u1', 'user', '\\/etc/hosts changed?', 0),
        row(
          'a1',
          'assistant',
          JSON.stringify({
            summary: 'The pool ran out.',
            evidence: [{ label: 'Pool at 100%', kind: 'metrics' }],
            likely_causes: ['A deploy halved the pool'],
            limitations: [],
            suggested_next_steps: ['Raise the pool size'],
          }),
          1,
        ),
        row('t1', 'tool', '{}', 2),
      ],
      LABELS,
    );

    expect(markdown).toContain('# Checkout latency');
    expect(markdown).toContain('## You · 2026-05-23 09:00 UTC\n\n/etc/hosts changed?');
    expect(markdown).toContain('## Mole Agent · 2026-05-23 09:01 UTC');
    expect(markdown).toContain('**Conclusion**\n\nThe pool ran out.');
    expect(markdown).toContain('- Pool at 100%');
    expect(markdown).toContain('**Likely causes**\n\n- A deploy halved the pool');
    expect(markdown).toContain('- Raise the pool size');
    expect(markdown).not.toContain('Limitations');
    expect(markdown).not.toContain('{}');
  });

  it('names the file after the chat and the day', () => {
    const day = Date.UTC(2026, 4, 23);
    expect(exportFileName('Why is checkout / slow?', day)).toBe('Why-is-checkout-slow-2026-05-23.md');
    expect(exportFileName('  ', day)).toBe('mole-agent-chat-2026-05-23.md');
  });
});
