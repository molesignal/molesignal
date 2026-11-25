import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { NodeAgentAssets } from '@/api/nodeAgent';
import i18n from '@/i18n';
import type * as productAccess from '@/product/access';
import { TooltipProvider } from '@/shell/ui/tooltip';
import { useAuthStore } from '@/stores/auth';
import type { AlertRule } from '@/types/alerting';

const mocks = vi.hoisted(() => ({
  assets: vi.fn(),
  install: vi.fn(),
  getRule: vi.fn(),
  updateRule: vi.fn(),
  permissions: new Set<string>(),
}));

vi.mock('@/api/apiTokens', () => ({
  getDefault: async () => ({ token: 'mspat-test0123456789secret', policyId: 'builtin:data_intake', tokenKind: 'default_intake' }),
  getRumClient: vi.fn(),
  scopes: async () => ({ policies: [], permissions: [] }),
}));
vi.mock('@/api/instance', () => ({ get: async () => ({ externalUrl: 'https://obs.example.com' }) }));
vi.mock('@/api/nodeAgent', () => ({ assets: mocks.assets, install: mocks.install }));
vi.mock('@/api/alerts', () => ({ get: mocks.getRule, update: mocks.updateRule }));
vi.mock('../codeBlock/highlighter', () => ({
  highlightCode: vi.fn(() => new Promise(() => {})),
}));
vi.mock('@/product/access', async (importOriginal) => ({
  ...(await importOriginal<typeof productAccess>()),
  useProductAccess: () => ({
    organizationId: 'acme',
    role: 'admin',
    scope: 'organization',
    permissions: mocks.permissions,
    organizationPermissions: {},
    version: 1,
    routeCatalogVersion: 0,
    routes: [],
    status: 'ready',
  }),
}));

import { Guide } from '../guide/Guide';
import { SOURCES } from '../sources';

function assets(overrides: Partial<NodeAgentAssets> = {}): NodeAgentAssets {
  return {
    streams: { metrics: true, events: true },
    indexes: { state: 'missing', fields: ['host.id'] },
    dashboard: { id: 'host-overview-acme', state: 'missing' },
    views: [{ key: 'oom', id: 'v-oom', installed: false }],
    alerts: [{ key: 'oom_kill', severity: 'error', ruleId: null, enabled: false }],
    ...overrides,
  };
}

const INSTALLED = assets({
  indexes: { state: 'ready' },
  dashboard: { id: 'host-overview-acme', state: 'current' },
  views: [{ key: 'oom', id: 'v-oom', installed: true }],
  alerts: [{ key: 'oom_kill', severity: 'error', ruleId: 'rule-1', enabled: false }],
});

function renderGuide() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  render(
    <QueryClientProvider client={client}>
      <TooltipProvider delayDuration={0}>
        <MemoryRouter>
          <Guide
            source={SOURCES.find((source) => source.id === 'node-agent')!}
            signal={undefined}
            streamName="default"
            overview={undefined}
            refreshOverview={vi.fn()}
          />
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const sections = () =>
  Array.from(document.querySelectorAll('[data-guide-section]'), (section) => section.getAttribute('data-guide-section'));
const asset = (key: string) => document.querySelector(`[data-node-agent-asset="${key}"]`);

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  for (const mock of [mocks.assets, mocks.install, mocks.getRule, mocks.updateRule]) mock.mockReset();
  mocks.permissions.clear();
  for (const key of ['dashboards.create', 'alerts.rules.manage', 'alerts.rules.read']) mocks.permissions.add(key);
  useAuthStore.setState({
    ctx: { userId: 'u1', orgId: 'acme', orgName: 'Acme', displayRole: 'admin', roles: [] },
  });
  vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, status: 200 })));
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  useAuthStore.setState({ ctx: null });
});

describe('the Node Agent guide', () => {
  it('sends to the gRPC port and the HTTP API, and adds the host assets and the runbook', async () => {
    mocks.assets.mockResolvedValue(assets());
    renderGuide();

    await waitFor(() => expect(asset('dashboard')).not.toBeNull());
    expect(sections()).toEqual(['1', '2', '3', '4', '5']);

    const text = document.querySelector('[data-datasource-guide="node-agent"]')!.textContent ?? '';

    expect(text).toContain('MS_ENDPOINT=https://obs.example.com:4317');
    expect(text).toContain('MS_HTTP_ENDPOINT=https://obs.example.com');
    expect(text).toContain('molesignal-node-agent doctor');
    expect(asset('indexes')?.getAttribute('data-state')).toBe('pending');
    expect(asset('indexes')?.textContent).toContain('no index yet on host.id');
    expect(document.querySelectorAll('[data-node-agent-runbook] [data-guide-step]')).toHaveLength(6);
    expect(within(document.querySelector('[data-node-agent-alert="oom_kill"]') as HTMLElement).getByText('Install the assets first')).not.toBeNull();
  });

  it('installs what is missing with one click and then offers to open it', async () => {
    mocks.assets.mockResolvedValue(assets());
    mocks.install.mockResolvedValue(INSTALLED);
    renderGuide();

    fireEvent.click(await screen.findByRole('button', { name: 'Install' }));

    await waitFor(() => expect(asset('dashboard')?.getAttribute('data-state')).toBe('ready'));
    expect(mocks.install).toHaveBeenCalledTimes(1);
    expect(within(asset('dashboard') as HTMLElement).getByRole('link', { name: 'Open' }).getAttribute('href')).toBe(
      '/dashboards/host-overview-acme',
    );
    expect(screen.getByRole('button', { name: 'Installed' })).toBeDisabled();
  });

  it('switches an alert rule on with one click', async () => {
    const rule = { id: 'rule-1', orgId: 'acme', name: 'OOM kill', enabled: false } as AlertRule;

    mocks.assets.mockResolvedValue(INSTALLED);
    mocks.getRule.mockResolvedValue(rule);
    mocks.updateRule.mockImplementation(async (_id: string, input: AlertRule) => ({ ...rule, ...input }));
    renderGuide();

    const enable = await screen.findByRole('button', { name: 'Enable' });
    const line = enable.closest('[data-node-agent-alert]') as HTMLElement;

    expect(line.getAttribute('data-node-agent-alert')).toBe('oom_kill');

    fireEvent.click(enable);

    await waitFor(() => expect(line.getAttribute('data-enabled')).toBe('true'));
    expect(mocks.updateRule).toHaveBeenCalledWith('rule-1', expect.objectContaining({ enabled: true }));
    expect(within(line).getByRole('link', { name: 'Edit' }).getAttribute('href')).toBe('/alerts/rules/rule-1/edit');
  });

  it('cannot install without the permission', async () => {
    mocks.permissions.clear();
    mocks.assets.mockResolvedValue(assets({ alerts: null }));
    renderGuide();

    expect(await screen.findByRole('button', { name: 'Install' })).toBeDisabled();
    expect(asset('alerts')?.getAttribute('data-state')).toBe('blocked');
  });
});
