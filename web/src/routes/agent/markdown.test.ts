import { act, render } from '@testing-library/react';
import * as React from 'react';
import { describe, expect, it, vi } from 'vitest';

import {
  MarkdownMessage,
  normalizeMarkdownSource,
  parseMarkdownBlocks,
} from './markdown';

describe('parseMarkdownBlocks', () => {
  it('parses headings, lists, and code fences', () => {
    const blocks = parseMarkdownBlocks([
      '## Summary',
      '',
      '- first',
      '- second',
      '',
      '```sql',
      'select * from logs',
      '```',
    ].join('\n'));

    expect(blocks).toEqual([
      { type: 'heading', level: 2, text: 'Summary' },
      { type: 'list', ordered: false, items: ['first', 'second'] },
      { type: 'code', lang: 'sql', code: 'select * from logs' },
    ]);
  });

  it('parses markdown tables', () => {
    const blocks = parseMarkdownBlocks([
      '| metric | value |',
      '| --- | ---: |',
      '| errors | 42 |',
    ].join('\n'));

    expect(blocks).toEqual([
      {
        type: 'table',
        headers: ['metric', 'value'],
        rows: [['errors', '42']],
      },
    ]);
  });

  it('keeps raw html as text content', () => {
    const blocks = parseMarkdownBlocks('<script>alert(1)</script>');
    expect(blocks).toEqual([{ type: 'paragraph', text: '<script>alert(1)</script>' }]);
  });

  it('repairs a block heading appended after prose', () => {
    const blocks = parseMarkdownBlocks(
      '好的，再次查询当前最新值班信息： ## 当前生产环境值班人员',
    );

    expect(blocks).toEqual([
      { type: 'paragraph', text: '好的，再次查询当前最新值班信息：' },
      { type: 'heading', level: 2, text: '当前生产环境值班人员' },
    ]);
  });

  it('does not normalize heading-like text inside fenced code', () => {
    const content = ['```text', '响应： ## 这不是标题', '```'].join('\n');
    expect(normalizeMarkdownSource(content)).toBe(content);
  });

  it('wraps long code block lines instead of horizontal scrolling', () => {
    const longRegex = String.raw`parse_regex!(.message, r'^(?P<remote_addr>\S+) - (?P<remote_user>\S+) \[(?P<time_local>[^\]]+)\] "(?P<request>[^"]*)")`;
    const { container } = render(React.createElement(MarkdownMessage, {
      content: ['```vrl', longRegex, '```'].join('\n'),
    }));

    const pre = container.querySelector('pre');
    expect(pre?.className).toContain('whitespace-pre-wrap');
    expect(pre?.className).toContain('[overflow-wrap:anywhere]');
    expect(pre?.className).not.toContain('overflow-x-auto');
  });

  it('renders RCA-style bold sections, lists, code, and safe links', () => {
    const { container } = render(React.createElement(MarkdownMessage, {
      content: [
        '**摘要**',
        '',
        '检测到 `http_requests_total` 异常。',
        '',
        '- 检查原始 Counter',
        '- 查看 [运行手册](https://example.com/runbook)',
      ].join('\n'),
    }));

    expect(container.querySelector('strong')?.textContent).toBe('摘要');
    expect(container.querySelector('code')?.textContent).toBe('http_requests_total');
    expect(container.querySelectorAll('li')).toHaveLength(2);
    expect(container.querySelector('a')?.getAttribute('href')).toBe('https://example.com/runbook');
  });

  it('lets a streamed answer in at an even pace, its newest characters bleeding in', () => {
    vi.useFakeTimers({ toFake: ['requestAnimationFrame', 'cancelAnimationFrame', 'performance'] });
    try {
      const answer = '错误率在 09:37 升到 1.94%，先回滚 checkout 2.0.0。';
      const { container, rerender } = render(
        React.createElement(MarkdownMessage, { content: '', streaming: true }),
      );
      rerender(React.createElement(MarkdownMessage, { content: answer, streaming: true }));
      expect(container.textContent).toBe('');

      act(() => {
        vi.advanceTimersByTime(16);
      });
      const first = container.textContent ?? '';
      expect(first.length).toBeGreaterThan(0);
      expect(first.length).toBeLessThan(answer.length);
      expect(container.querySelector('.ms-ink')?.textContent).toBeTruthy();

      act(() => {
        vi.advanceTimersByTime(2000);
      });
      expect(container.textContent).toBe(answer);
      expect(container.querySelector('.ms-ink')).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('shows a message that does not stream whole, with nothing bleeding in', () => {
    const { container } = render(React.createElement(MarkdownMessage, { content: '已完成。' }));
    expect(container.textContent).toBe('已完成。');
    expect(container.querySelector('.ms-ink')).toBeNull();
  });
});
