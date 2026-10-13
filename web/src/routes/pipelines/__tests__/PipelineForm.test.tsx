import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { PipelineInput, ScheduledPipeline } from '@/api/pipelines';
import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

vi.mock('@/api/connectors', () => ({
  list: async () => [
    { id: 'c1', name: 'Archive', kind: 's3' },
    // A connector that brings logs in: not somewhere a pipeline can deliver to.
    { id: 'c2', name: 'Edge requests', kind: 'cloudflare_logpush' },
  ],
}));
vi.mock('@/api/functions', () => ({ list: async () => [], run: vi.fn() }));
vi.mock('@/api/streams', () => ({
  list: async () => [
    { name: 'app_logs', stream_type: 'logs' },
    { name: 'audit_logs', stream_type: 'logs' },
    { name: 'cpu', stream_type: 'metrics' },
  ],
}));
vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({ allowed: true, disabled: false, reason: undefined }),
}));
vi.mock('@/shell/ui/select', () => import('./nativeSelect'));
// The stream picker is a field like any other here; what it offers is its own test.
vi.mock('../editor/StreamCombobox', () => ({
  StreamCombobox: ({
    value,
    streams,
    creatable,
    onChange,
  }: {
    value: string;
    streams: readonly string[];
    creatable?: boolean;
    onChange: (stream: string) => void;
  }) => (
    <input
      data-streams={streams.join(',')}
      data-creatable={creatable ? 'true' : 'false'}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));
// The code editor is a field like any other here.
vi.mock('@/shell/codeEditor', () => ({
  CodeEditor: ({
    value,
    onChange,
    ariaLabel,
  }: {
    value: string;
    onChange?: (value: string) => void;
    ariaLabel?: string;
  }) => (
    <textarea
      aria-label={ariaLabel}
      value={value}
      onChange={(event) => onChange?.(event.target.value)}
    />
  ),
}));

import { STARTER_VRL } from '../editor/steps';
import { PipelineForm } from '../PipelineForm';

type Steps = { steps: Array<Record<string, unknown>> } & Record<string, unknown>;

const SAVED: ScheduledPipeline = {
  id: 'p1',
  name: 'nightly',
  source_stream: 'app_logs',
  target_stream: 'clean_logs',
  cron: 'every:2h',
  lookback_secs: 90,
  enabled: false,
  function_steps: { steps: [{ transform_name: 'keep', script: '. = .' }] },
};

function renderForm(initial?: ScheduledPipeline) {
  const submit = vi.fn<(payload: PipelineInput) => void>();
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  render(
    <QueryClientProvider client={query}>
      <TooltipProvider delayDuration={0}>
        <PipelineForm formId="pipeline" initial={initial ?? null} onSubmit={submit} />
        <button type="submit" form="pipeline">
          Save
        </button>
      </TooltipProvider>
    </QueryClientProvider>,
  );

  return {
    submit,
    save: () => fireEvent.click(screen.getByRole('button', { name: 'Save' })),
    payload: () => submit.mock.calls.at(-1)?.[0] as PipelineInput,
    steps: () => (submit.mock.calls.at(-1)?.[0].function_steps as Steps).steps,
  };
}

const node = (id: string) => document.querySelector<HTMLElement>(`[data-pipeline-node="${id}"]`)!;
/** What each card of a lane reads: its number, its name, what it does. */
const nodes = (kind: string) =>
  Array.from(document.querySelectorAll(`[data-pipeline-node^="${kind}-"]`), (card) =>
    Array.from(card.querySelectorAll('*'))
      .filter((part) => part.childElementCount === 0 && part.textContent?.trim())
      .map((part) => part.textContent?.trim())
      .join(' '),
  );
const editor = () => within(document.querySelector<HTMLElement>('[data-pipeline-node-editor]')!);
const settings = () => within(document.querySelector<HTMLElement>('[data-pipeline-settings]')!);
const status = () => document.querySelector('[data-flow-status]')?.getAttribute('data-flow-status');
const statusText = () => document.querySelector('[data-flow-status]')?.textContent;
/** How each card is marked: `pending` for something left to fill in, `error`, `warning`, or nothing. */
const marks = () =>
  Object.fromEntries(
    Array.from(document.querySelectorAll<HTMLElement>('[data-pipeline-node]'), (card) => [
      card.dataset.pipelineNode,
      card.querySelector('[data-flow-node-issue]')?.getAttribute('data-flow-node-issue') ?? null,
    ]),
  );
const issues = () =>
  Array.from(document.querySelectorAll('[data-flow-issue]'), (item) => [
    item.getAttribute('data-flow-issue'),
    item.textContent?.trim(),
  ]);
const heading = () => editor().getByRole('heading', { level: 2 }).textContent;
const sourceField = () => editor().getByRole<HTMLInputElement>('textbox', { name: /^Source stream/ });
const destinationField = () => editor().getByRole<HTMLInputElement>('textbox', { name: /^Destination stream/ });
const type = (field: HTMLElement, value: string) => fireEvent.change(field, { target: { value } });

/** Names the pipeline and chooses what it reads and writes, leaving the source open. */
function fillIn() {
  type(screen.getByRole('textbox', { name: 'Name' }), 'tidy');
  fireEvent.click(node('sink-0'));
  type(destinationField(), 'clean_logs');
  fireEvent.click(node('source-0'));
  type(sourceField(), 'app_logs');
}

async function addStep(user: ReturnType<typeof userEvent.setup>, kind: string) {
  await user.click(screen.getByRole('button', { name: 'Add step' }));
  await user.click(await screen.findByRole('menuitem', { name: kind }));
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  Element.prototype.scrollIntoView = vi.fn();
});

afterEach(cleanup);

describe('pipeline editor', () => {
  it('opens a new pipeline on its source, with what it has to be told left to fill in', () => {
    renderForm();

    expect(nodes('source')).toEqual(['Choose a stream Stream input']);
    expect(nodes('transform')).toEqual([]);
    expect(nodes('sink')).toEqual(['Choose a stream Stream output']);
    expect(node('source-0').getAttribute('aria-pressed')).toBe('true');
    expect(heading()).toBe('Source');
    // Work to do, not a mistake: it is counted and marked on the cards, which say what to do
    // themselves, and nothing is listed or red before a save is tried.
    expect(status()).toBe('pending');
    expect(statusText()).toBe('2 left to fill in');
    expect(marks()).toEqual({ 'source-0': 'pending', 'sink-0': 'pending' });
    expect(issues()).toEqual([]);
    expect(node('source-0').getAttribute('aria-invalid')).toBeNull();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('holds a save back until the pipeline says what it reads and writes', () => {
    const { save, submit, payload } = renderForm();
    type(screen.getByRole('textbox', { name: 'Name' }), 'copy');

    save();

    expect(submit).not.toHaveBeenCalled();
    expect(status()).toBe('error');
    expect(screen.getByRole('alert').textContent).toContain('Fix these before saving');
    expect(issues()).toEqual([
      ['error', 'Choose the stream to read.'],
      ['error', 'Choose the stream to write to.'],
    ]);
    expect(node('source-0').getAttribute('aria-invalid')).toBe('true');

    type(sourceField(), 'app_logs');
    fireEvent.click(screen.getByRole('button', { name: 'Choose the stream to write to.' }));
    expect(node('sink-0').getAttribute('aria-pressed')).toBe('true');
    type(destinationField(), 'clean_logs');

    expect(status()).toBe('valid');
    expect(screen.queryByRole('alert')).toBeNull();
    save();

    // With no steps it copies what it reads.
    expect(payload()).toMatchObject({ name: 'copy', source_stream: 'app_logs', target_stream: 'clean_logs' });
    expect(payload().function_steps).toMatchObject({ sources: ['app_logs'], sinks: ['clean_logs'], steps: [] });
  });

  it('reads from a stream the workspace has and may write to one it does not have yet', async () => {
    renderForm();

    await waitFor(() => expect(sourceField().getAttribute('data-streams')).toBe('app_logs,audit_logs'));
    expect(sourceField().getAttribute('data-creatable')).toBe('false');

    fireEvent.click(node('sink-0'));
    expect(destinationField().getAttribute('data-creatable')).toBe('true');
    type(destinationField(), 'brand_new');
    expect(nodes('sink')).toEqual(['brand_new Stream output']);
    expect(statusText()).toBe('1 left to fill in');
    expect(marks()).toEqual({ 'source-0': 'pending', 'sink-0': null });

    // A name that could not be a stream is held back.
    type(destinationField(), 'brand new');
    expect(issues()).toContainEqual([
      'error',
      '"brand new" is not a stream name: use letters, numbers, _ - and . only.',
    ]);
  });

  it('will not write back into the stream it reads', () => {
    const { save, submit } = renderForm();
    fillIn();
    fireEvent.click(node('sink-0'));
    type(destinationField(), 'app_logs');

    expect(status()).toBe('error');
    expect(issues()).toEqual([
      ['error', 'The pipeline would write to app_logs, the stream it reads. Choose another destination.'],
    ]);
    save();
    expect(submit).not.toHaveBeenCalled();
  });

  it('saves the schedule the way the backend runs it', () => {
    const { save, payload } = renderForm();
    fillIn();

    type(settings().getByRole('spinbutton', { name: 'Run every: number' }), '15');
    type(settings().getByRole('combobox', { name: 'Run every: unit' }), 's');
    type(settings().getByRole('spinbutton', { name: 'Read the last: number' }), '2');
    type(settings().getByRole('combobox', { name: 'Read the last: unit' }), 'h');
    save();

    expect(payload()).toMatchObject({ name: 'tidy', cron: 'every:15s', lookback_secs: 7200, enabled: true });
    // Nothing on the page claims to set how a failed run is retried: nothing reads it.
    expect(settings().queryByText(/retry/i)).toBeNull();
  });

  it('reads a saved pipeline back: its schedule in counts and units, its first step picked', () => {
    renderForm(SAVED);

    expect(settings().getByRole<HTMLInputElement>('spinbutton', { name: 'Run every: number' }).value).toBe('2');
    expect(settings().getByRole<HTMLSelectElement>('combobox', { name: 'Run every: unit' }).value).toBe('h');
    expect(
      settings().getByRole<HTMLInputElement>('spinbutton', { name: 'Read the last: number' }).value,
    ).toBe('90');
    expect(
      settings().getByRole<HTMLSelectElement>('combobox', { name: 'Read the last: unit' }).value,
    ).toBe('s');
    expect(settings().getByRole('switch').getAttribute('aria-checked')).toBe('false');
    expect(nodes('source')).toEqual(['app_logs Stream input']);
    expect(nodes('transform')).toEqual(['1 keep Custom VRL']);
    expect(nodes('sink')).toEqual(['clean_logs Stream output']);
    expect(node('transform-0').getAttribute('aria-pressed')).toBe('true');
    expect(status()).toBe('valid');
  });

  it('builds a chain of processors and saves each as the VRL it stands for', async () => {
    const user = userEvent.setup();
    const { save, steps } = renderForm();
    fillIn();

    await addStep(user, 'Rename field');
    expect(node('transform-0').getAttribute('aria-pressed')).toBe('true');
    type(editor().getByRole('textbox', { name: /^Field/ }), 'msg');
    type(editor().getByRole('textbox', { name: 'New name' }), 'message');
    await addStep(user, 'Mask field');
    type(editor().getByRole('textbox', { name: /^Field/ }), 'token');

    expect(nodes('transform')).toEqual(['1 rename Rename field · msg → message', '2 mask Mask field · token']);
    save();

    expect(steps()).toEqual([
      {
        transform_name: 'rename',
        kind: 'vrl',
        script: 'if exists(.msg) {\n  .message = del(.msg)\n}',
        processor: { op: 'rename', from: 'msg', to: 'message' },
      },
      {
        transform_name: 'mask',
        kind: 'vrl',
        script: 'if exists(.token) {\n  .token = "***"\n}',
        processor: { op: 'mask', field: 'token', mode: 'full' },
      },
    ]);
  });

  it('holds a save back while a step is unfinished, and leads to the step', async () => {
    const user = userEvent.setup();
    const { save, submit } = renderForm(SAVED);
    await addStep(user, 'Rename field');
    fireEvent.click(node('transform-0'));

    // Just added: something to fill in, shown quietly.
    expect(status()).toBe('pending');
    expect(node('transform-1').getAttribute('aria-invalid')).toBeNull();
    save();

    expect(submit).not.toHaveBeenCalled();
    expect(screen.getByRole('alert').textContent).toContain('Fix these before saving');
    expect(issues()).toEqual([['error', 'Step "rename" still has an empty field.']]);
    expect(node('transform-1').getAttribute('aria-invalid')).toBe('true');

    fireEvent.click(screen.getByRole('button', { name: 'Step "rename" still has an empty field.' }));
    expect(node('transform-1').getAttribute('aria-pressed')).toBe('true');

    type(editor().getByRole('textbox', { name: /^Field/ }), 'a');
    type(editor().getByRole('textbox', { name: 'New name' }), 'b');
    expect(status()).toBe('valid');
    expect(screen.queryByRole('alert')).toBeNull();
    save();

    expect(submit).toHaveBeenCalledTimes(1);
  });

  it('moves a step earlier or later and keeps it picked', async () => {
    const user = userEvent.setup();
    const { save, steps } = renderForm(SAVED);
    await addStep(user, 'Parse JSON');

    expect(editor().getByRole('button', { name: 'Move later' })).toBeDisabled();
    fireEvent.click(editor().getByRole('button', { name: 'Move earlier' }));

    expect(nodes('transform')).toEqual(['1 parse-json Parse JSON · message', '2 keep Custom VRL']);
    expect(node('transform-0').getAttribute('aria-pressed')).toBe('true');
    expect(editor().getByRole('button', { name: 'Move earlier' })).toBeDisabled();
    save();

    expect(steps().map((step) => step.transform_name)).toEqual(['parse-json', 'keep']);
  });

  it('removes steps down to none, and never the source or the stream', async () => {
    const user = userEvent.setup();
    const { save, steps } = renderForm(SAVED);
    await addStep(user, 'Parse JSON');

    fireEvent.click(editor().getByRole('button', { name: 'Remove' }));
    expect(nodes('transform')).toEqual(['1 keep Custom VRL']);
    expect(node('transform-0').getAttribute('aria-pressed')).toBe('true');

    fireEvent.click(editor().getByRole('button', { name: 'Remove' }));
    expect(nodes('transform')).toEqual([]);
    // With no step left the source is what is open, and it is not something to remove.
    expect(node('source-0').getAttribute('aria-pressed')).toBe('true');
    expect(editor().queryByRole('button', { name: 'Remove' })).toBeNull();
    fireEvent.click(node('sink-0'));
    expect(editor().queryByRole('button', { name: 'Remove' })).toBeNull();

    expect(status()).toBe('valid');
    save();
    expect(steps()).toEqual([]);
  });

  it('turns a step into another kind, and a processor into VRL to be edited by hand', async () => {
    const user = userEvent.setup();
    const { save, steps } = renderForm(SAVED);
    await addStep(user, 'Change case');
    type(editor().getByRole('textbox', { name: /^Field/ }), 'level');

    fireEvent.click(editor().getByRole('button', { name: 'Show generated VRL' }));
    expect(editor().getByLabelText('Generated VRL').textContent).toBe(
      'if is_string(.level) {\n  .level = downcase(string!(.level))\n}',
    );
    fireEvent.click(editor().getByRole('button', { name: 'Edit as VRL' }));

    expect(editor().getByRole<HTMLSelectElement>('combobox', { name: 'Processing type' }).value).toBe('vrl');
    expect(editor().getByRole<HTMLTextAreaElement>('textbox', { name: 'VRL script' }).value).toBe(
      'if is_string(.level) {\n  .level = downcase(string!(.level))\n}',
    );
    save();
    expect(steps()[1]).toEqual({
      transform_name: 'transform',
      kind: 'vrl',
      script: 'if is_string(.level) {\n  .level = downcase(string!(.level))\n}',
    });

    type(editor().getByRole('combobox', { name: 'Processing type' }), 'remove');
    expect(editor().getByRole('textbox', { name: /^Fields/ })).not.toBeNull();
    expect(nodes('transform')[1]).toBe('2 remove Remove fields');
  });

  it('starts a hand-written step from a note that runs and changes nothing', async () => {
    const user = userEvent.setup();
    const { save, steps } = renderForm(SAVED);

    await addStep(user, 'Custom VRL');

    expect(editor().getByRole<HTMLTextAreaElement>('textbox', { name: 'VRL script' }).value).toBe(STARTER_VRL);
    expect(status()).toBe('valid');

    type(editor().getByRole('textbox', { name: 'VRL script' }), '.seen = true');
    type(editor().getByRole('textbox', { name: 'Step name' }), 'mark');
    save();
    expect(steps()[1]).toEqual({ transform_name: 'mark', kind: 'vrl', script: '.seen = true' });
  });

  it('delivers to connectors beside the stream, each one chosen and not guessed', async () => {
    const { save, submit, payload } = renderForm(SAVED);

    fireEvent.click(screen.getByRole('button', { name: 'Add connector' }));

    expect(nodes('sink')).toEqual(['clean_logs Stream output', 'Choose a connector Connector delivery']);
    expect(node('sink-1').getAttribute('aria-pressed')).toBe('true');
    expect(heading()).toBe('Connector');
    expect(marks()['sink-1']).toBe('pending');
    save();
    expect(submit).not.toHaveBeenCalled();
    expect(issues()).toEqual([['error', 'Choose a connector for this destination.']]);

    await waitFor(() =>
      expect(
        Array.from(
          editor().getByRole<HTMLSelectElement>('combobox', { name: /^Connector/ }).options,
          (option) => option.textContent,
        ),
      ).toEqual(['', 'Archive (s3)']),
    );
    type(editor().getByRole('combobox', { name: /^Connector/ }), 'c1');

    expect(nodes('sink')).toEqual(['clean_logs Stream output', 'Archive Connector delivery']);
    expect(heading()).toBe('ConnectorArchive');
    expect(status()).toBe('valid');
    save();
    expect(payload().function_steps).toMatchObject({ sinks: ['clean_logs'], sink_connectors: ['c1'] });

    fireEvent.click(editor().getByRole('button', { name: 'Remove' }));
    expect(nodes('sink')).toEqual(['clean_logs Stream output']);
    save();
    expect(payload().function_steps).toMatchObject({ sink_connectors: [] });
  });

  it('says of names an earlier editor saved that nothing reads or writes them, and lets them go', () => {
    const { save, payload } = renderForm({
      ...SAVED,
      function_steps: {
        sources: ['audit_logs', 'app_logs'],
        sinks: ['clean_logs', 'archive_logs'],
        steps: [],
      },
    });

    // What runs leads; what does not is marked as such.
    expect(nodes('source')).toEqual(['app_logs Stream input', 'audit_logs Saved, never read']);
    expect(nodes('sink')).toEqual(['clean_logs Stream output', 'archive_logs Saved, never written']);
    expect(screen.queryByRole('button', { name: /Add source/ })).toBeNull();
    expect(status()).toBe('warning');
    expect(issues()).toEqual([
      ['warning', 'audit_logs is never read: a pipeline reads one stream, app_logs.'],
      ['warning', 'Nothing is written to archive_logs: events go to clean_logs.'],
    ]);

    fireEvent.click(screen.getByRole('button', { name: /audit_logs is never read/ }));
    expect(editor().getByText(/audit_logs is saved with it but is never read/)).not.toBeNull();
    fireEvent.click(editor().getByRole('button', { name: 'Remove' }));
    fireEvent.click(screen.getByRole('button', { name: /Nothing is written to archive_logs/ }));
    fireEvent.click(editor().getByRole('button', { name: 'Remove' }));

    expect(status()).toBe('valid');
    save();
    expect(payload()).toMatchObject({ source_stream: 'app_logs', target_stream: 'clean_logs' });
    expect(payload().function_steps).toMatchObject({ sources: ['app_logs'], sinks: ['clean_logs'] });
  });

  it('asks for the streams again when the signal changes, since streams belong to a signal', async () => {
    renderForm(SAVED);

    type(settings().getByRole('combobox', { name: 'Signal type' }), 'metrics');

    expect(nodes('source')).toEqual(['Choose a stream Stream input']);
    expect(nodes('sink')).toEqual(['Choose a stream Stream output']);
    expect(nodes('transform')).toEqual(['1 keep Custom VRL']);
    fireEvent.click(node('source-0'));
    await waitFor(() => expect(sourceField().getAttribute('data-streams')).toBe('cpu'));
  });

  it('works on logs as they arrive in realtime mode: one stream in, one out, no schedule', async () => {
    const user = userEvent.setup();
    const { save, payload, steps } = renderForm();
    fillIn();
    fireEvent.click(screen.getByRole('button', { name: 'Add connector' }));

    fireEvent.click(settings().getByRole('button', { name: 'Realtime' }));

    expect(settings().queryByText('Run every')).toBeNull();
    expect(settings().getByRole('combobox', { name: 'Signal type' })).toBeDisabled();
    expect(screen.queryByRole('button', { name: 'Add connector' })).toBeNull();
    // The connector went with the mode; a realtime source may be a stream that is yet to come.
    expect(nodes('sink')).toEqual(['clean_logs Stream output']);
    fireEvent.click(node('source-0'));
    expect(sourceField().getAttribute('data-creatable')).toBe('true');

    await addStep(user, 'Route to stream');
    // Routing is by a field, and which field is for its author to say.
    expect(editor().getByRole('button', { name: 'Route by field' }).getAttribute('aria-pressed')).toBe('true');
    expect(marks()['transform-0']).toBe('pending');
    save();
    expect(issues()).toEqual([['error', 'Step "route" needs a field and a fallback stream.']]);
    type(editor().getByRole('textbox', { name: /^Field name/ }), 'appname');
    expect(nodes('sink')).toEqual(['Stream output Chosen by the routing step']);

    fireEvent.click(node('source-0'));
    fireEvent.click(editor().getByRole('checkbox', { name: 'Also retain in source stream' }));
    save();

    expect(payload()).toMatchObject({ cron: '', source_stream: 'app_logs', target_stream: 'default' });
    expect(payload().function_steps).toMatchObject({
      mode: 'realtime',
      retain_source: true,
      sources: ['app_logs'],
      sinks: ['default'],
      sink_connectors: [],
    });
    expect(steps().at(-1)).toMatchObject({
      kind: 'builtin',
      operation: 'route',
      routing: { kind: 'field', field: 'appname', fallback: 'default' },
    });

    fireEvent.click(settings().getByRole('button', { name: 'Scheduled' }));
    expect(settings().getByText('Run every')).not.toBeNull();
  });

  it('will not route a scheduled run back into the stream it reads', async () => {
    const user = userEvent.setup();
    const { save, submit } = renderForm(SAVED);

    await addStep(user, 'Route to stream');
    fireEvent.click(editor().getByRole('button', { name: 'Fixed stream' }));
    type(editor().getByRole('textbox', { name: /^Destination stream/ }), 'app_logs');

    expect(issues()).toEqual([
      [
        'error',
        'Step "route" would route events back into app_logs, the stream this pipeline reads. Every run would fail.',
      ],
    ]);
    save();
    expect(submit).not.toHaveBeenCalled();
  });
});
