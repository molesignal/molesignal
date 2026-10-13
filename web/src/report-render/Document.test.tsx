import { cleanup, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { ReportDocument, ReportUnavailable } from './Document';

afterEach(cleanup);

describe('ReportDocument', () => {
  it('says what the report is and what it covers before the report itself', () => {
    render(
      <ReportDocument
        title="Checkout health"
        description="How the checkout path is doing."
        facts={[
          { label: 'Time range', value: '2026-10-04 00:00 – 2026-10-11 00:00' },
          { label: 'Generated', value: '2026-10-11 09:30' },
        ]}
      >
        <p>the panels</p>
      </ReportDocument>,
    );

    const header = screen.getByRole('banner');
    expect(within(header).getByRole('heading', { level: 1, name: 'Checkout health' })).toBeTruthy();
    expect(within(header).getByText('How the checkout path is doing.')).toBeTruthy();
    expect(
      within(header)
        .getAllByRole('term')
        .map((term) => term.textContent),
    ).toEqual(['Time range', 'Generated']);
    expect(
      within(header)
        .getAllByRole('definition')
        .map((definition) => definition.textContent),
    ).toEqual(['2026-10-04 00:00 – 2026-10-11 00:00', '2026-10-11 09:30']);

    // The content follows the header; it is not inside it.
    expect(within(header).queryByText('the panels')).toBeNull();
    expect(screen.getByText('the panels')).toBeTruthy();
  });

  it('keeps facts that share a label apart', () => {
    render(
      <ReportDocument
        title="Fleet"
        facts={[
          { label: 'Region', value: 'us-east-1' },
          { label: 'Region', value: 'eu-west-1' },
        ]}
      >
        <p>the panels</p>
      </ReportDocument>,
    );

    expect(screen.getAllByRole('definition').map((definition) => definition.textContent)).toEqual([
      'us-east-1',
      'eu-west-1',
    ]);
  });

  it('draws no list of facts when there are none', () => {
    const { container } = render(
      <ReportDocument title="Fleet">
        <p>the panels</p>
      </ReportDocument>,
    );

    expect(container.querySelector('dl')).toBeNull();
  });
});

describe('ReportUnavailable', () => {
  it('tells the reader and the renderer the same reason', () => {
    render(<ReportUnavailable reason="Dashboard not found." />);

    expect(screen.getByRole('alert').textContent).toBe('Dashboard not found.');
    expect(document.documentElement.dataset.reportRenderState).toBe('error');
    expect(document.documentElement.dataset.reportRenderError).toBe('Dashboard not found.');
  });
});
