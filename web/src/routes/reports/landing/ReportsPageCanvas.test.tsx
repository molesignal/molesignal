import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import '@/i18n';

import { ReportsPageCanvas } from './ReportsPageCanvas';

describe('ReportsPageCanvas', () => {
  it('uses one flat canvas for KPIs, tabs, controls, and content', () => {
    const onTabChange = vi.fn();
    const { container } = render(
      <MemoryRouter initialEntries={['/reports']}>
        <ReportsPageCanvas
          title="Reports"
          subtitle="Report operations"
          toolbar={<button type="button">New report</button>}
          kpis={[
            { label: 'Running', value: '4', tone: 'good' },
            { label: 'Failed', value: '0' },
          ]}
          tabs={[
            { id: 'schedules', label: 'Schedules', count: 4 },
            { id: 'history', label: 'History', count: 2 },
            { id: 'templates', label: 'Templates' },
          ]}
          activeTab="schedules"
          onTabChange={onTabChange}
          actionBar={<div>Filters</div>}
        >
          <div>Report rows</div>
        </ReportsPageCanvas>
      </MemoryRouter>,
    );

    const kpis = container.querySelector('[data-reports-kpis]');
    expect(kpis).not.toBeNull();
    expect(kpis?.className).not.toMatch(/rounded|shadow|border-y|divide-x/);
    expect(kpis?.className).toMatch(/\bborder-b\b/);

    const tablist = screen.getByRole('tablist', { name: 'Reports' });
    expect(tablist.parentElement?.className).not.toMatch(
      /rounded|shadow|border-y/,
    );
    expect(tablist.parentElement?.className).not.toMatch(/\bborder-b\b/);
    const activeTab = screen.getByRole('tab', { name: 'Schedules 4' });
    expect(activeTab.className).toContain('border-b-[3px]');
    expect(activeTab.className).not.toContain('-mb-px');
    expect(screen.getByText('Filters').parentElement?.className).not.toMatch(
      /rounded|shadow|border-y/,
    );

    fireEvent.click(screen.getByRole('tab', { name: 'Templates' }));
    expect(onTabChange).toHaveBeenCalledWith('templates');
    expect(screen.getByText('Report rows')).toBeInTheDocument();
  });
});
