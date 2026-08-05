import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { TemplateMessagePreview } from './MessagePreview';

describe('TemplateMessagePreview', () => {
  it('renders Markdown structure instead of its source', () => {
    const { container } = render(
      <TemplateMessagePreview
        format="markdown"
        content={'**Alert**\n\n- service: `api`'}
        emptyText="Empty"
      />,
    );

    expect(container.querySelector('strong')?.textContent).toBe('Alert');
    expect(container.querySelector('li')?.textContent).toContain(
      'service: api',
    );
    expect(container.textContent).not.toContain('**Alert**');
  });

  it('renders sanitized HTML structure', () => {
    const { container } = render(
      <TemplateMessagePreview
        format="html"
        content={'<h3>Alert</h3><p>Status: <strong>open</strong></p><script>alert(1)</script>'}
        emptyText="Empty"
      />,
    );

    expect(container.querySelector('h3')?.textContent).toBe('Alert');
    expect(container.querySelector('strong')?.textContent).toBe('open');
    expect(container.querySelector('script')).toBeNull();
  });

  it('keeps text messages literal', () => {
    const { container } = render(
      <TemplateMessagePreview
        format="text"
        content="<strong>Alert</strong>"
        emptyText="Empty"
      />,
    );

    expect(container.querySelector('pre')?.textContent).toBe(
      '<strong>Alert</strong>',
    );
    expect(container.querySelector('strong')).toBeNull();
  });
});
