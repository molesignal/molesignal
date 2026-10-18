import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { liveEditingLoses, MarkdownEditor } from './MarkdownEditor';

const runbook = `# CEX

入口文件，用 **@路径** 列出各文件。

- 交易 API
- 撮合引擎
  - 订单簿

1. 第一步
2. 第二步

- [ ] 补全负责团队
- [x] 写入服务清单

> 未确认的内容标为草稿。

\`\`\`bash
kubectl get pods -n cex
\`\`\`

| 环境 | 说明 |
| --- | --- |
| uat | 验收 |

看 [Grafana](https://grafana.example.com) 与 \`trace_id\`。
`;

describe('liveEditingLoses', () => {
  it('keeps what a runbook is written with', () => {
    expect(liveEditingLoses(runbook)).toBe(false);
    expect(liveEditingLoses('')).toBe(false);
  });

  it('notices what it would drop', () => {
    expect(liveEditingLoses(`${runbook}\n<!-- 内部备注 -->\n`)).toBe(true);
  });
});

describe('MarkdownEditor', () => {
  afterEach(cleanup);

  it('shows the Markdown as it reads and reports nothing until it is changed', async () => {
    const onChange = vi.fn();
    render(<MarkdownEditor value={runbook} onChange={onChange} ariaLabel="内容" />);

    const field = await screen.findByRole('textbox', { name: '内容' });
    await waitFor(() => expect(field.querySelector('h1')?.textContent).toBe('CEX'));
    expect(field.querySelectorAll('ul li').length).toBeGreaterThan(2);
    expect(field.querySelector('ul[data-type="taskList"]')).not.toBeNull();
    expect(field.querySelector('table')).not.toBeNull();
    expect(field.querySelector('pre code')?.textContent).toContain('kubectl');
    expect(onChange).not.toHaveBeenCalled();
  });

  it('takes a value set from outside', async () => {
    const { rerender } = render(<MarkdownEditor value="- one" onChange={() => {}} ariaLabel="内容" />);
    const field = await screen.findByRole('textbox', { name: '内容' });
    await waitFor(() => expect(field.querySelector('li')?.textContent).toBe('one'));

    rerender(<MarkdownEditor value="## two" onChange={() => {}} ariaLabel="内容" />);
    await waitFor(() => expect(field.querySelector('h2')?.textContent).toBe('two'));
  });
});
