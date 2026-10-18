import '@/i18n';

import { focusManager, QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as providersApi from '@/api/agent/modelProviders';
import type { ModelProvider } from '@/api/agent/modelProviders';
import * as inspectionsApi from '@/api/logInspections';
import type { InspectionLimits, LogInspection, LogInspectionRun } from '@/api/logInspections';
import * as notifyApi from '@/api/notify';
import * as streamsApi from '@/api/streams';
import i18n from '@/i18n';
import { setActiveTimePreference } from '@/lib/time';
import type * as productAccess from '@/product/access';
import type { ProductAccess } from '@/product/access';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { InspectionEditor } from './Editor';
import { ScheduleInspectionButton } from './Entry';
import { LogInspections } from './index';

vi.mock('@/api/logInspections', async (importOriginal) => ({
  ...(await importOriginal<typeof inspectionsApi>()),
  list: vi.fn(),
  limits: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  setEnabled: vi.fn(),
  remove: vi.fn(),
  runNow: vi.fn(),
}));
vi.mock('@/api/agent/modelProviders', async (importOriginal) => ({
  ...(await importOriginal<typeof providersApi>()),
  list: vi.fn(),
}));
vi.mock('@/api/notify', async (importOriginal) => ({
  ...(await importOriginal<typeof notifyApi>()),
  listConnectors: vi.fn(),
}));
vi.mock('@/api/streams', async (importOriginal) => ({
  ...(await importOriginal<typeof streamsApi>()),
  list: vi.fn(),
}));
vi.mock('@/shell/ui/sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const session = vi.hoisted(() => ({ access: null as unknown }));
vi.mock('@/product/access', async (importOriginal) => ({
  ...(await importOriginal<typeof productAccess>()),
  useProductAccess: () => session.access,
}));

const api = vi.mocked(inspectionsApi);
const providers = vi.mocked(providersApi);

const SECOND = 1_000_000;
const NOW = Date.UTC(2026, 9, 13, 2) * 1000;
const LIMITS: InspectionLimits = {
  min_interval_secs: 3_600,
  max_per_org: 20,
  run_timeout_secs: 300,
  report_retention_days: 30,
};
/** The label of the name field; it carries the mark of a field that must be filled. */
const NAME = /^Name/;
const MANAGER = ['agent.use', 'streams.query', 'inspections.manage', 'alerts.read'];
const READER = ['agent.use', 'streams.query'];

function access(permissions: string[]): ProductAccess {
  return {
    organizationId: 'acme-prod',
    role: 'Owner',
    scope: 'organization',
    permissions: new Set(permissions) as ProductAccess['permissions'],
    version: 1,
    routeCatalogVersion: 1,
    routes: [],
    status: 'ready',
  };
}

function inspection(id: string, overrides: Partial<LogInspection> = {}): LogInspection {
  return {
    id,
    name: `Inspection ${id}`,
    stream: 'app_logs',
    statement: "level = 'ERROR'",
    window_secs: 3_600,
    requirements: '',
    schedule: { type: 'interval', every_minutes: 60 },
    timezone: 'UTC',
    provider_id: 'provider-1',
    model: null,
    notify_when: 'abnormal',
    notify_routes: [{ connector_id: 'c1', target_type: 'fixed_group', target: 'oc_ops' }],
    language: 'en-us',
    enabled: true,
    run_as_user_id: 'u1',
    run_as_user: { id: 'u1', display_name: 'Dana Reyes', email: 'dana@example.com' },
    next_run_at_micros: NOW + 38 * 60 * SECOND,
    last_run: { at_micros: NOW - 22 * 60 * SECOND, status: 'succeeded', level: 'normal' },
    active_run: null,
    created_by: 'u1',
    created_at_micros: NOW - 86_400 * SECOND,
    updated_at_micros: NOW - 86_400 * SECOND,
    ...overrides,
  };
}

const PROVIDER: ModelProvider = {
  id: 'provider-1',
  provider: 'openai',
  name: 'OpenAI',
  default_model: 'gpt-4o',
  enabled: true,
  timeout_ms: 30_000,
  price_currency: 'USD',
  key_set: true,
  created_at_micros: 1,
  updated_at_micros: 1,
};

/** A backend holding `inspections`, answered from as it stands now. */
function serve(inspections: LogInspection[], limits: InspectionLimits = LIMITS) {
  const held = [...inspections];
  const change = (id: string, next: Partial<LogInspection>) => {
    const index = held.findIndex((item) => item.id === id);
    held[index] = { ...held[index]!, ...next };
    return held[index]!;
  };

  api.list.mockImplementation(async () => [...held]);
  api.limits.mockResolvedValue(limits);
  api.setEnabled.mockImplementation(async (id, enabled) =>
    change(id, { enabled, next_run_at_micros: enabled ? NOW + 3_600 * SECOND : null }),
  );
  api.runNow.mockImplementation(async (id) => {
    change(id, {
      active_run: {
        id: 'run-queued',
        status: 'queued',
        trigger: 'manual',
        created_at_micros: NOW,
        started_at_micros: null,
      },
    });

    return { id: 'run-queued', inspection_id: id, status: 'queued' } as LogInspectionRun;
  });
  api.remove.mockImplementation(async (id) => {
    held.splice(
      held.findIndex((item) => item.id === id),
      1,
    );
  });
}

function renderPage() {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TooltipProvider delayDuration={0}>
        <MemoryRouter initialEntries={['/logs/inspections']}>
          <LogInspections />
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const row = (id: string) => {
  const cell = document.querySelector<HTMLElement>(`[data-log-inspection="${id}"]`);
  if (!cell) throw new Error(`no row for ${id}`);
  return within(cell.closest('tr')!);
};
const listed = async (id: string) => waitFor(() => row(id));

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  vi.clearAllMocks();
  vi.spyOn(Date, 'now').mockReturnValue(NOW / 1000);
  setActiveTimePreference('UTC', 'iso_24h');
  session.access = access(MANAGER);
  providers.list.mockResolvedValue([PROVIDER]);
  vi.mocked(notifyApi.listConnectors).mockResolvedValue([]);
  vi.mocked(streamsApi.list).mockResolvedValue([]);
});

afterEach(() => {
  cleanup();
  focusManager.setFocused(undefined);
  setActiveTimePreference('', 'iso_24h');
  vi.restoreAllMocks();
});

describe('scheduled log inspections', () => {
  it('says of each inspection what it inspects, how often, how it last went and when it is next', async () => {
    serve([
      inspection('hourly'),
      inspection('weekly', {
        name: 'Weekly audit',
        statement: '',
        schedule: { type: 'weekly', weekday: 1, hour: 9, minute: 30 },
        window_secs: 7 * 86_400,
        notify_when: 'always',
        notify_routes: [],
        last_run: { at_micros: NOW - 3 * 86_400 * SECOND, status: 'succeeded', level: 'abnormal' },
      }),
    ]);
    renderPage();
    await listed('hourly');

    const hourly = row('hourly');
    expect(hourly.getByRole('link', { name: 'Inspection hourly' })).toHaveAttribute(
      'href',
      '/logs/inspections/hourly',
    );
    expect(hourly.getByText('app_logs')).toBeInTheDocument();
    expect(hourly.getByText(/level = 'ERROR'/)).toBeInTheDocument();
    expect(hourly.getByText('Every 1 hour')).toBeInTheDocument();
    expect(hourly.getByText('Covers the last 1 hour')).toBeInTheDocument();
    expect(hourly.getByText('Normal')).toBeInTheDocument();
    expect(hourly.getByText('22 minutes ago')).toBeInTheDocument();
    expect(hourly.getByText('in 38 minutes')).toBeInTheDocument();
    expect(hourly.getByText('Notify when abnormal')).toBeInTheDocument();
    expect(hourly.getByText('1 channel')).toBeInTheDocument();

    const weekly = row('weekly');
    expect(weekly.getByText('Mondays at 09:30')).toBeInTheDocument();
    expect(weekly.getByText('Covers the last 7 days')).toBeInTheDocument();
    expect(weekly.getByText('Abnormal')).toBeInTheDocument();
    // No query: every log of the stream. No channel: nobody is told.
    expect(weekly.getByText(/All logs/)).toBeInTheDocument();
    expect(weekly.getByText('No notifications')).toBeInTheDocument();

    expect(document.querySelector('[data-log-inspection-count]')).toHaveTextContent(
      '2 saved; the limit is 20',
    );
  });

  it('marks an inspection that never ran, one that failed, and one that is running', async () => {
    serve([
      inspection('new', { last_run: null }),
      inspection('broken', { last_run: { at_micros: NOW - 60 * SECOND, status: 'timed_out', level: null } }),
      inspection('busy', {
        active_run: {
          id: 'r9',
          status: 'running',
          trigger: 'schedule',
          created_at_micros: NOW - 30 * SECOND,
          started_at_micros: NOW - 30 * SECOND,
        },
      }),
      inspection('off', { enabled: false, next_run_at_micros: null }),
    ]);
    renderPage();
    await listed('new');

    expect(row('new').getByText('Not run yet')).toBeInTheDocument();
    expect(row('broken').getByText('Timed out')).toBeInTheDocument();
    expect(row('busy').getByText('Inspecting')).toBeInTheDocument();
    expect(row('off').getByText('Switched off')).toBeInTheDocument();

    // Something is under way: the page says it keeps itself up to date.
    expect(document.querySelector('[data-log-inspections-live]')).toBeInTheDocument();
    // A run cannot be started on top of another.
    expect(row('busy').getByRole('button', { name: 'Run Inspection busy now' })).toBeDisabled();
    expect(row('new').getByRole('button', { name: 'Run Inspection new now' })).toBeEnabled();
  });

  it('switches an inspection off and on from its row', async () => {
    serve([inspection('hourly')]);
    renderPage();
    await listed('hourly');

    fireEvent.click(row('hourly').getByRole('switch', { name: 'Switch Inspection hourly on or off' }));

    await waitFor(() => expect(api.setEnabled).toHaveBeenCalledWith('hourly', false));
    await waitFor(() => expect(row('hourly').getByText('Switched off')).toBeInTheDocument());
  });

  it('queues a run from the row and shows it as under way', async () => {
    serve([inspection('hourly')]);
    renderPage();
    await listed('hourly');

    fireEvent.click(row('hourly').getByRole('button', { name: 'Run Inspection hourly now' }));

    await waitFor(() => expect(api.runNow).toHaveBeenCalledWith('hourly'));
    await waitFor(() => expect(row('hourly').getByText('Queued')).toBeInTheDocument());
  });

  it('deletes an inspection only after it is confirmed', async () => {
    serve([inspection('hourly'), inspection('other')]);
    renderPage();
    await listed('hourly');

    fireEvent.click(row('hourly').getByRole('button', { name: 'Delete Inspection hourly' }));

    const dialog = within(await screen.findByRole('dialog'));
    expect(dialog.getByText('Delete “Inspection hourly”?')).toBeInTheDocument();
    expect(api.remove).not.toHaveBeenCalled();

    fireEvent.click(dialog.getByRole('button', { name: 'Delete' }));

    await waitFor(() => expect(api.remove).toHaveBeenCalledWith('hourly'));
    await waitFor(() =>
      expect(document.querySelector('[data-log-inspection="hourly"]')).not.toBeInTheDocument(),
    );
    expect(document.querySelector('[data-log-inspection="other"]')).toBeInTheDocument();
  });

  it('shows a reader the list with nothing they can change', async () => {
    session.access = access(READER);
    serve([inspection('hourly')]);
    renderPage();
    await listed('hourly');

    const hourly = row('hourly');
    expect(hourly.getByRole('switch')).toBeDisabled();
    expect(hourly.getByRole('button', { name: 'Run Inspection hourly now' })).toBeDisabled();
    expect(hourly.getByRole('button', { name: 'Edit Inspection hourly' })).toBeDisabled();
    expect(hourly.getByRole('button', { name: 'Delete Inspection hourly' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'New inspection' })).toBeDisabled();
  });

  it('says which permissions are missing instead of asking the backend', async () => {
    session.access = access(['streams.query']);
    serve([inspection('hourly')]);
    renderPage();

    expect(await screen.findByText('You cannot see scheduled inspections')).toBeInTheDocument();
    expect(screen.getByText(/agent\.use and streams\.query/)).toBeInTheDocument();
    expect(api.list).not.toHaveBeenCalled();
  });

  it('keeps the saved inspections on the page when no model is switched on, and says they cannot run', async () => {
    providers.list.mockResolvedValue([{ ...PROVIDER, enabled: false }]);
    serve([inspection('hourly')]);
    renderPage();
    await listed('hourly');

    const notice = await waitFor(() => {
      const found = document.querySelector<HTMLElement>('[data-log-inspections-no-model]');
      if (!found) throw new Error('no notice');
      return found;
    });
    expect(notice).toHaveTextContent('No AI model is switched on in this organization');
    // This reader cannot configure models: they are told whom to ask.
    expect(notice).toHaveTextContent('Ask an administrator');
    expect(screen.getByRole('button', { name: 'New inspection' })).toBeDisabled();
  });

  it('offers to save the first inspection where there is none', async () => {
    serve([]);
    renderPage();

    expect(await screen.findByText('No scheduled inspections yet')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'New inspection' }));

    const dialog = within(await screen.findByRole('dialog'));

    expect(screen.getByRole('dialog')).toHaveTextContent('New scheduled inspection');
    // It takes the first model there is, and waits for a name and a stream.
    await waitFor(() =>
      expect(dialog.getByRole('combobox', { name: 'AI model' })).toHaveTextContent('OpenAI · gpt-4o'),
    );
    expect(dialog.getByRole('combobox', { name: 'Stream' })).toHaveTextContent('Choose a log stream');
    expect(dialog.getByRole('button', { name: 'Save inspection' })).toBeDisabled();
  });

  it('opens an inspection in the form with all it was saved with, ready to be saved again', async () => {
    serve([inspection('one')]);
    renderPage();
    (await listed('one')).getByRole('button', { name: 'Edit Inspection one' }).click();

    const dialog = within(await screen.findByRole('dialog'));

    // Its stream is the one choice of the field until the streams are listed, and it stays chosen.
    await waitFor(() => expect(streamsApi.list).toHaveBeenCalled());
    expect(dialog.getByRole('combobox', { name: 'Stream' })).toHaveTextContent('app_logs');
    await waitFor(() =>
      expect(dialog.getByRole('combobox', { name: 'AI model' })).toHaveTextContent('OpenAI · gpt-4o'),
    );
    expect(dialog.getByLabelText(NAME)).toHaveValue('Inspection one');
    expect(dialog.getByRole('button', { name: 'Save' })).toBeEnabled();
  });

  it('keeps what was typed when the page behind the form reads the inspection again', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const form = (read: LogInspection) => (
      <QueryClientProvider client={client}>
        <TooltipProvider delayDuration={0}>
          <InspectionEditor open onOpenChange={() => undefined} inspection={read} limits={LIMITS} />
        </TooltipProvider>
      </QueryClientProvider>
    );
    const { rerender } = render(form(inspection('one')));
    const name = await screen.findByLabelText(NAME);

    fireEvent.change(name, { target: { value: 'Renamed while a run finished' } });
    // The same inspection, read again: a run of it ended in the meantime.
    rerender(
      form(inspection('one', { last_run: { at_micros: NOW, status: 'succeeded', level: 'abnormal' } })),
    );

    expect(screen.getByLabelText(NAME)).toHaveValue('Renamed while a run finished');
    expect(screen.getByRole('combobox', { name: 'Stream' })).toHaveTextContent('app_logs');
  });

  it('starts every opening of the form from what it was opened for', async () => {
    serve([inspection('one'), inspection('two', { name: 'Second', stream: 'audit_logs' })]);
    renderPage();
    (await listed('one')).getByRole('button', { name: 'Edit Inspection one' }).click();

    const first = within(await screen.findByRole('dialog'));

    fireEvent.change(first.getByLabelText(NAME), { target: { value: 'Typed and given up' } });
    fireEvent.click(first.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());

    row('two').getByRole('button', { name: 'Edit Second' }).click();

    const second = within(await screen.findByRole('dialog'));

    expect(second.getByLabelText(NAME)).toHaveValue('Second');
    expect(second.getByRole('combobox', { name: 'Stream' })).toHaveTextContent('audit_logs');
  });

  it('drafts the inspection of a query within what the instance allows', async () => {
    // This instance runs an inspection every two hours at the most.
    let answer: (limits: InspectionLimits) => void = () => undefined;
    api.limits.mockReturnValue(new Promise((resolve) => (answer = resolve)));
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <TooltipProvider delayDuration={0}>
          <ScheduleInspectionButton
            seed={{ stream: 'app_logs', statement: "level = 'ERROR'", providerId: 'provider-1' }}
          />
        </TooltipProvider>
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Save as scheduled inspection' }));
    await waitFor(() => expect(api.limits).toHaveBeenCalled());
    // The form is not drawn on a guess of the limits.
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();

    answer({ ...LIMITS, min_interval_secs: 7_200 });

    const dialog = within(await screen.findByRole('dialog'));

    expect(dialog.getByRole('combobox', { name: 'Interval' })).toHaveTextContent('2 hours');
    expect(dialog.getByRole('combobox', { name: 'Period covered' })).toHaveTextContent('3 hours');
    expect(dialog.getByRole('combobox', { name: 'Stream' })).toHaveTextContent('app_logs');
  });

  it('shows a channel whose connector was disabled since, for what it is', async () => {
    vi.mocked(notifyApi.listConnectors).mockResolvedValue([
      {
        id: 'c1',
        organization_id: 'org',
        name: 'Lark · Ops',
        connector_type: 'lark_app',
        config: {},
        capabilities: {
          direct_user: true,
          group: true,
          rich_text: true,
          interactive: false,
          acknowledgement: false,
          attachments: false,
        },
        enabled: false,
        status: 'connected',
        created_at: 1,
        updated_at: 1,
      },
    ]);
    serve([inspection('one')]);
    renderPage();
    (await listed('one')).getByRole('button', { name: 'Edit Inspection one' }).click();

    const dialog = within(await screen.findByRole('dialog'));

    // The channel is neither hidden nor shown as if it still delivered, and it can be taken out.
    expect(await dialog.findByLabelText('Target of channel 1')).toHaveValue('oc_ops');
    expect(dialog.getByRole('combobox', { name: 'Connector of channel 1' })).toHaveTextContent(
      'Lark · Ops (disabled)',
    );
    expect(dialog.getByRole('button', { name: 'Remove channel' })).toBeInTheDocument();
    expect(document.querySelector('[data-inspection-channels]')).toBeNull();
  });

  it('says in the form that the model of the inspection was disabled since', async () => {
    providers.list.mockResolvedValue([
      { ...PROVIDER, enabled: false },
      { ...PROVIDER, id: 'provider-2', name: 'Qwen', default_model: 'qwen-max' },
    ]);
    serve([inspection('one')]);
    renderPage();
    (await listed('one')).getByRole('button', { name: 'Edit Inspection one' }).click();

    const dialog = within(await screen.findByRole('dialog'));

    await waitFor(() =>
      expect(dialog.getByRole('combobox', { name: 'AI model' })).toHaveTextContent(
        'OpenAI · gpt-4o (disabled)',
      ),
    );
    expect(screen.getByRole('dialog')).toHaveTextContent(
      'This model is disabled. The inspection cannot be saved or run until the model is enabled again or another one is chosen.',
    );
  });

  it('says the channels are kept as they are to someone who may not see the connectors', async () => {
    session.access = access(MANAGER.filter((permission) => permission !== 'alerts.read'));
    serve([inspection('one')]);
    renderPage();
    (await listed('one')).getByRole('button', { name: 'Edit Inspection one' }).click();
    await screen.findByRole('dialog');

    expect(document.querySelector('[data-inspection-channels="denied"]')).toHaveTextContent(
      'Choosing a notification channel requires the alerts.read permission. The 1 channel already chosen is kept as it is.',
    );
    expect(notifyApi.listConnectors).not.toHaveBeenCalled();
  });

  it('stops offering a new inspection once the organization holds as many as it may', async () => {
    serve([inspection('one'), inspection('two')], { ...LIMITS, max_per_org: 2 });
    renderPage();
    await listed('one');

    expect(screen.getByRole('button', { name: 'New inspection' })).toBeDisabled();
    expect(document.querySelector('[data-log-inspection-count]')).toHaveTextContent(
      '2 saved; the limit is 2',
    );
  });
});
