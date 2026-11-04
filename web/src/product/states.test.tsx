import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { AxiosError, AxiosHeaders } from 'axios';
import { afterEach, describe, expect, it, vi } from 'vitest';

import '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { ProductState } from './states';

afterEach(cleanup);

describe('ProductState', () => {
  it('shows loading as the pulse travelling a line, not a spinner', () => {
    const { container } = render(<ProductState variant="loading" title="Loading" />);
    expect(screen.getByRole('status')).not.toBeNull();
    expect(container.querySelector('path.ms-pulse-travel')).not.toBeNull();
    expect(container.querySelector('.animate-spin')).toBeNull();
  });

  it('shows empty as a flat line with one beat', () => {
    const { container } = render(<ProductState variant="empty" title="Nothing yet" />);
    expect(container.querySelector('path.ms-pulse-draw')).not.toBeNull();
  });

  it('keeps a tinted glyph for the states that need attention', () => {
    for (const [variant, tile] of [
      ['error', 'bg-red-dim'],
      ['permission-denied', 'bg-red-dim'],
      ['backend-pending', 'bg-[var(--yellow-tint)]'],
    ] as const) {
      const { container, unmount } = render(<ProductState variant={variant} title="Attention" />);
      // By attribute: a class that names a variable (`bg-[var(--x)]`) is not a valid `.class` selector.
      expect(container.querySelector(`[class~="${tile}"] svg`)).not.toBeNull();
      expect(container.querySelector('.ms-pulse-draw')).toBeNull();
      unmount();
    }
  });

  it('states the status and the request id of a failure, ready to copy', () => {
    const error = new AxiosError('failed', '500', undefined, undefined, {
      status: 500,
      statusText: 'Internal Server Error',
      data: { message: 'query engine unavailable' },
      headers: new AxiosHeaders({ 'x-request-id': 'req_7c41e9a2' }),
      config: { headers: new AxiosHeaders() },
    });
    render(
      <TooltipProvider>
        <ProductState variant="error" error={error} />
      </TooltipProvider>,
    );

    expect(screen.getByText('query engine unavailable')).not.toBeNull();
    expect(screen.getByText('HTTP 500')).not.toBeNull();
    expect(screen.getByText('req_7c41e9a2')).not.toBeNull();
    expect(screen.getByRole('button', { name: 'Copy request ID' })).not.toBeNull();
  });

  it('has nothing to look up for a failure that never reached the server', () => {
    const { container } = render(<ProductState variant="error" error={new Error('bad link')} />);
    expect(container.querySelector('[data-error-reference]')).toBeNull();
  });

  it('offers to retry the failed queries of the page by itself', async () => {
    const queryFn = vi.fn().mockRejectedValueOnce(new Error('down')).mockResolvedValue('fine');
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    function Page() {
      const query = useQuery({ queryKey: ['list'], queryFn });
      if (query.isPending) return null;
      return query.isError ? <ProductState variant="error" error={query.error} /> : <p>{query.data}</p>;
    }

    render(
      <QueryClientProvider client={client}>
        <Page />
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole('button', { name: 'Retry' }));

    await waitFor(() => expect(screen.getByText('fine')).not.toBeNull());
    expect(queryFn).toHaveBeenCalledTimes(2);
  });

  it('offers no retry where there is nothing to repeat', () => {
    render(<ProductState variant="error" error={new Error('bad link')} />);
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  });

  it('keeps the action a caller gives instead of its own retry', () => {
    const client = new QueryClient();
    client.getQueryCache().build(client, { queryKey: ['x'] });
    render(
      <QueryClientProvider client={client}>
        <ProductState variant="error" error={new Error('x')} action={<button type="button">Do this</button>} />
      </QueryClientProvider>,
    );
    expect(screen.getByRole('button', { name: 'Do this' })).not.toBeNull();
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  });

  it('draws a retry the caller asks for beside its other actions', () => {
    const onRetry = vi.fn();
    render(
      <ProductState
        variant="error"
        error={new Error('x')}
        onRetry={onRetry}
        action={<a href="/docs">Docs</a>}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('link', { name: 'Docs' })).not.toBeNull();
  });

  it('announces errors as alerts and everything else as status', () => {
    const { rerender } = render(<ProductState variant="error" title="Broken" />);
    expect(screen.getByRole('alert')).not.toBeNull();
    rerender(<ProductState variant="empty" title="Nothing yet" />);
    expect(screen.getByRole('status')).not.toBeNull();
  });
});
