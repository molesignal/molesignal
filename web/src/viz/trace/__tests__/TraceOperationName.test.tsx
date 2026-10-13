import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  parseTraceOperation,
  TraceOperationName,
} from '@/viz/trace/TraceOperationName';

describe('TraceOperationName', () => {
  it('splits and normalizes a standard HTTP operation', () => {
    expect(parseTraceOperation('post /api/checkout')).toEqual({
      method: 'POST',
      target: '/api/checkout',
    });
  });

  it('renders the HTTP method as a metric-style semantic prefix', () => {
    render(<TraceOperationName operation="POST /api/checkout" />);

    const method = screen.getByText('POST', { selector: '[aria-hidden="true"]' });
    expect(method.className).toContain('type-micro');
    expect(method.className).toContain('font-mono');
    // An ink tone, not a soft one: soft is a fill on light and falls short of AA as text.
    expect(method.className).toContain('text-[var(--blue-ink)]');
    expect(method.className).not.toContain('-soft');
    expect(screen.getByText('/api/checkout')).toBeTruthy();
  });

  it('leaves non-HTTP span operations unchanged', () => {
    render(<TraceOperationName operation="checkout.create" />);

    expect(parseTraceOperation('checkout.create')).toBeNull();
    expect(screen.getByText('checkout.create')).toBeTruthy();
  });
});
