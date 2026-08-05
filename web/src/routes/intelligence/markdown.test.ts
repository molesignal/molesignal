import { render } from '@testing-library/react';
import * as React from 'react';
import { describe, expect, it } from 'vitest';

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
});
