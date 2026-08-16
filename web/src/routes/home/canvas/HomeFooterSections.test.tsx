import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createDashboardPanel } from '@/dashboard-engine/factories';
import {
  createEmptyDashboardDefinition,
  dashboardDefinitionToModel,
} from '@/dashboard-engine/model';
import i18n from '@/i18n';
import type { ActivationState } from '@/product/activation';
import type { Dashboard } from '@/types/dashboard';

import { ActivationStrip } from './ActivationStrip';
import { RecentDashboardsSection } from './RecentDashboardsSection';

const NOW_MICROS = 1_800_000_000_000_000;

function dashboard({
  id,
  title,
  panels,
  updatedAt,
}: {
  id: string;
  title: string;
  panels: number;
  updatedAt: number;
}): Dashboard {
  const definition = createEmptyDashboardDefinition(title);
  definition.elements = Array.from({ length: panels }, () =>
    createDashboardPanel(definition.elements),
  );

  return {
    id,
    org_id: 'org-1',
    uid: id,
    title,
    tags: [],
    model: dashboardDefinitionToModel(definition),
    version: 1,
    created_at: updatedAt,
    updated_at: updatedAt,
  };
}

function renderDashboards(
  dashboards: Dashboard[],
  callbacks = {
    onOpenDashboard: vi.fn(),
    onAddPanels: vi.fn(),
    onCreateDashboard: vi.fn(),
  },
) {
  render(
    <RecentDashboardsSection
      dashboards={dashboards}
      loading={false}
      onOpenDashboard={callbacks.onOpenDashboard}
      onAddPanels={callbacks.onAddPanels}
      onCreateDashboard={callbacks.onCreateDashboard}
      createDashboardDisabled={false}
      editDashboardDisabled={false}
      onViewAll={vi.fn()}
    />,
  );
  return callbacks;
}

afterEach(cleanup);

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

describe('RecentDashboardsSection', () => {
  it('shows only dashboards with panels and orders them by recency', () => {
    const callbacks = renderDashboards([
      dashboard({
        id: 'older',
        title: 'Older overview',
        panels: 2,
        updatedAt: NOW_MICROS - 2_000,
      }),
      dashboard({
        id: 'empty',
        title: 'Empty dashboard',
        panels: 0,
        updatedAt: NOW_MICROS,
      }),
      dashboard({
        id: 'newer',
        title: 'Newer overview',
        panels: 3,
        updatedAt: NOW_MICROS - 1_000,
      }),
    ]);

    const rows = screen.getAllByTestId('recent-dashboard-row');
    expect(rows).toHaveLength(2);
    expect(rows[0]?.textContent).toContain('Newer overview');
    expect(rows[1]?.textContent).toContain('Older overview');
    expect(screen.queryByText('Empty dashboard')).toBeNull();

    fireEvent.click(rows[0]!);
    expect(callbacks.onOpenDashboard).toHaveBeenCalledWith('newer');
  });

  it('offers to add panels when existing dashboards are empty', () => {
    const callbacks = renderDashboards([
      dashboard({
        id: 'empty',
        title: 'Empty dashboard',
        panels: 0,
        updatedAt: NOW_MICROS,
      }),
    ]);

    expect(screen.getByText('No dashboards with panels yet')).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Add panels' }));
    expect(callbacks.onAddPanels).toHaveBeenCalledWith('empty');
  });

  it('offers dashboard creation when the workspace has none', () => {
    const callbacks = renderDashboards([]);

    expect(screen.getByText('No dashboards yet')).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'New dashboard' }));
    expect(callbacks.onCreateDashboard).toHaveBeenCalledOnce();
  });
});

describe('ActivationStrip', () => {
  const incompleteState: ActivationState = {
    completedCount: 4,
    totalCount: 5,
    ready: true,
    steps: [
      {
        id: 'datasource',
        labelKey: 'steps.datasource.title',
        descriptionKey: 'steps.datasource.description',
        completed: true,
        to: '/datasource',
      },
      {
        id: 'sample-data',
        labelKey: 'steps.sample_data.title',
        descriptionKey: 'steps.sample_data.description',
        completed: false,
        to: '/datasource/recommended/http-json',
      },
    ],
  };

  it('names the remaining step and opens setup', () => {
    const onOpen = vi.fn();
    render(<ActivationStrip state={incompleteState} onOpen={onOpen} />);

    expect(screen.getByText('1 setup step remaining')).not.toBeNull();
    expect(screen.getByText('Load sample data')).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Complete setup' }));
    expect(onOpen).toHaveBeenCalledOnce();
  });

  it('hides after every setup step is complete', () => {
    const completeState: ActivationState = {
      ...incompleteState,
      completedCount: 5,
      steps: incompleteState.steps.map((step) => ({
        ...step,
        completed: true,
      })),
    };

    render(<ActivationStrip state={completeState} onOpen={vi.fn()} />);
    expect(screen.queryByRole('status')).toBeNull();
  });
});
