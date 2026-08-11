import '@testing-library/jest-dom/vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as alertsApi from '@/api/alerts';
import * as syntheticsApi from '@/api/synthetics';
import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';
import type { AlertRule } from '@/types/alerting';

import { SourceResourceField } from './SourceResourceField';

vi.mock('@/api/alerts', () => ({ list: vi.fn() }));
vi.mock('@/api/synthetics', () => ({
  getMonitor: vi.fn(),
  listMonitors: vi.fn(),
}));

const listAlertRules = vi.mocked(alertsApi.list);
const listMonitors = vi.mocked(syntheticsApi.listMonitors);

function renderField(sourceKind: 'alert_incident' | 'synthetic_monitor' = 'alert_incident') {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <SourceResourceField
          sourceKind={sourceKind}
          value=""
          onChange={vi.fn()}
          allowAll={false}
          required
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

function alertRule(): AlertRule {
  return {
    id: 'rule-api-latency',
    org_id: 'org-one',
    name: 'API latency',
    description: '',
    enabled: true,
    kind: 'scheduled',
    query: {
      language: 'promql',
      statement: 'up == 0',
      period_secs: 60,
      stream: { name: 'http_server', stream_type: 'metrics' },
    },
    trigger: {
      operator: 'gt',
      threshold: 0,
      for_periods: 1,
      silence_secs: 0,
    },
    escalation_policy_id: '',
    labels: {},
    annotations: {},
  };
}

describe('SourceResourceField', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    listAlertRules.mockReset();
    listMonitors.mockReset();
  });

  afterEach(() => cleanup());

  it('replaces the loading placeholder once alert rules are available', async () => {
    listAlertRules.mockResolvedValue([alertRule()]);

    renderField();

    const select = screen.getByRole('combobox', { name: /Alert rule/ });
    expect(select).toHaveTextContent('Loading sources…');
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: /Alert rule/ })).toHaveTextContent(
        'Select an alert rule',
      );
    });
    expect(screen.getByRole('combobox', { name: /Alert rule/ })).toBeEnabled();
  });

  it('shows the empty state instead of a loading placeholder', async () => {
    listAlertRules.mockResolvedValue([]);

    renderField();

    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: /Alert rule/ })).toHaveTextContent(
        'No alert rules available',
      );
    });
    expect(screen.getByRole('combobox', { name: /Alert rule/ })).toBeEnabled();
  });

  it('uses a source-specific placeholder for synthetic checks', async () => {
    listMonitors.mockResolvedValue([]);

    renderField('synthetic_monitor');

    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: /Synthetic check/ })).toHaveTextContent(
        'No published synthetic checks available',
      );
    });
  });
});
