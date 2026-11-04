import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  parseTraceOperation,
  TraceOperationName,
} from '@/viz/trace/TraceOperationName';

describe('TraceOperationName', () => {
  it('splits and normalizes a standard HTTP operation', () => {
    expect(parseTraceOperation('post /api/checkout')).toEqual({
      verb: 'POST',
      action: 'add',
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

  it('writes the verb as a word of the row in a list table', () => {
    render(<TraceOperationName operation="GET /cart" mono={false} />);

    const method = screen.getByText('GET', { selector: '[aria-hidden="true"]' });
    expect(method.className).not.toContain('font-mono');
    expect(method.className).not.toContain('type-micro');
    expect(method.className).toContain('text-[var(--green-ink)]');
  });

  it('leads SQL statements and Redis or MongoDB commands with their verb, toned by what it does', () => {
    expect(parseTraceOperation('SELECT orders')).toEqual({ verb: 'SELECT', action: 'read', target: 'orders' });
    expect(parseTraceOperation('insert into orders\nvalues ($1)')).toEqual({
      verb: 'INSERT',
      action: 'add',
      target: 'into orders\nvalues ($1)',
    });
    expect(parseTraceOperation('hgetall session:1')?.action).toBe('read');
    expect(parseTraceOperation('SET cart:9')?.action).toBe('change');
    expect(parseTraceOperation('DEL cart:9')?.action).toBe('remove');
    expect(parseTraceOperation('COMMIT')).toEqual({ verb: 'COMMIT', action: 'control', target: '' });
    expect(parseTraceOperation('find orders')?.action).toBe('read');

    render(<TraceOperationName operation="DELETE FROM carts" />);
    const verb = screen.getByText('DELETE', { selector: '[aria-hidden="true"]' });
    expect(verb.className).toContain('text-[var(--red-ink)]');
    expect(screen.getByText('FROM carts')).toBeTruthy();
  });

  it('leaves other span operations unchanged', () => {
    render(<TraceOperationName operation="checkout.create" />);

    expect(parseTraceOperation('checkout.create')).toBeNull();
    expect(parseTraceOperation('process orders')).toBeNull();
    expect(parseTraceOperation('publish orders')).toBeNull();
    expect(parseTraceOperation('grpc.health.v1.Health/Check')).toBeNull();
    expect(screen.getByText('checkout.create')).toBeTruthy();
  });
});
