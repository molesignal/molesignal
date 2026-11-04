import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type * as React from 'react';
import { createPortal } from 'react-dom';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { DataTable } from '@/admin';
import i18n from '@/i18n';
import { isNestedControlClick } from '@/shell/lib/nestedControl';
import { MetricsLegend } from '@/shell/MetricsLegend';
import { SignalReference } from '@/shell/SignalReference';

afterEach(() => cleanup());

/** What `isNestedControlClick` says of each click the container hears. */
function renderContainer(children: React.ReactNode, props: React.HTMLAttributes<HTMLDivElement> = {}) {
  const verdicts: boolean[] = [];
  render(
    <div data-testid="container" {...props} onClick={(event) => verdicts.push(isNestedControlClick(event))}>
      {children}
    </div>,
  );
  return verdicts;
}

describe('isNestedControlClick', () => {
  it('tells a click on a control inside the container from one on the rest of it', () => {
    const verdicts = renderContainer(
      <>
        <span>a span</span>
        <button type="button">
          <svg data-testid="icon" />
          fold
        </button>
        {/* jsdom does not navigate: the link keeps its click to itself. */}
        <a href="/logs" onClick={(event) => event.preventDefault()}>
          logs
        </a>
        <span role="img" aria-label="Leaf span">
          o
        </span>
      </>,
    );

    fireEvent.click(screen.getByText('a span'));
    fireEvent.click(screen.getByTestId('icon'));
    fireEvent.click(screen.getByRole('link', { name: 'logs' }));
    fireEvent.click(screen.getByRole('img', { name: 'Leaf span' }));
    fireEvent.click(screen.getByTestId('container'));

    expect(verdicts).toEqual([false, true, true, false, false]);
  });

  it('does not count the container itself when it is a control, as a React Flow node is', () => {
    const verdicts = renderContainer(
      <>
        <span>checkout</span>
        <button type="button">reference</button>
      </>,
      { role: 'button', tabIndex: 0 },
    );

    fireEvent.click(screen.getByText('checkout'));
    fireEvent.click(screen.getByRole('button', { name: 'reference' }));

    expect(verdicts).toEqual([false, true]);
  });

  it('does not count a control the container stands in', () => {
    const verdicts: boolean[] = [];
    render(
      <div role="button" tabIndex={0}>
        <div data-testid="container" onClick={(event) => verdicts.push(isNestedControlClick(event))}>
          <span>inside</span>
        </div>
      </div>,
    );

    fireEvent.click(screen.getByText('inside'));

    expect(verdicts).toEqual([false]);
  });

  it('counts a click in a menu opened from inside the container, which React passes up through its portal', () => {
    const verdicts = renderContainer(
      <>
        <span>a span</span>
        {createPortal(<span>in a menu</span>, document.body)}
      </>,
    );

    fireEvent.click(screen.getByText('in a menu'));
    fireEvent.click(screen.getByText('a span'));

    expect(verdicts).toEqual([true, false]);
  });
});

describe('containers with references in them', () => {
  it('opens a reference in a table row without opening the row', async () => {
    await i18n.changeLanguage('en-us');
    const user = userEvent.setup();
    const onRowClick = vi.fn();
    render(
      <MemoryRouter>
        <DataTable
          rows={[{ id: 'trace-1', service: 'checkout' }]}
          rowKey={(row) => row.id}
          onRowClick={onRowClick}
          columns={[
            { key: 'id', header: 'Trace', cell: (row) => row.id },
            {
              key: 'service',
              header: 'Service',
              cell: (row) => <SignalReference type="service" value={row.service} />,
            },
          ]}
        />
      </MemoryRouter>,
    );

    await user.click(screen.getByRole('button', { name: /checkout/ }));
    expect(screen.getByRole('dialog')).toHaveTextContent('checkout');
    expect(onRowClick).not.toHaveBeenCalled();

    await user.click(screen.getByText('trace-1'));
    expect(onRowClick).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('follows a jump from the menu of a reference in a table row without opening the row', async () => {
    await i18n.changeLanguage('en-us');
    const user = userEvent.setup();
    const onRowClick = vi.fn();
    render(
      <MemoryRouter>
        <DataTable
          rows={[{ id: 'trace-1', service: 'checkout' }]}
          rowKey={(row) => row.id}
          onRowClick={onRowClick}
          columns={[
            { key: 'id', header: 'Trace', cell: (row) => row.id },
            {
              key: 'service',
              header: 'Service',
              cell: (row) => <SignalReference type="service" value={row.service} />,
            },
          ]}
        />
      </MemoryRouter>,
    );

    await user.click(screen.getByRole('button', { name: /checkout/ }));
    await user.click(within(screen.getByRole('dialog')).getByRole('link', { name: /View logs for this service/ }));

    expect(screen.queryByRole('dialog')).toBeNull();
    expect(onRowClick).not.toHaveBeenCalled();
  });

  it('opens a reference in a legend row without hiding or soloing its series', async () => {
    await i18n.changeLanguage('en-us');
    const user = userEvent.setup();
    const onToggleIndex = vi.fn();
    const onSoloIndex = vi.fn();
    render(
      <MemoryRouter>
        <MetricsLegend
          series={[{ labels: { service: 'checkout', region: 'eu' }, values: [1], timestamps: [1], valueColumn: 'value' }]}
          colors={['red']}
          metricName="http_requests_total"
          onToggleIndex={onToggleIndex}
          onSoloIndex={onSoloIndex}
        />
      </MemoryRouter>,
    );

    await user.dblClick(screen.getByRole('button', { name: /checkout/ }));
    expect(onToggleIndex).not.toHaveBeenCalled();
    expect(onSoloIndex).not.toHaveBeenCalled();

    await user.click(screen.getByText('http_requests_total'));
    expect(onToggleIndex).toHaveBeenCalledWith(0);
    await user.dblClick(screen.getByText('http_requests_total'));
    expect(onSoloIndex).toHaveBeenCalledWith(0);
  });
});
