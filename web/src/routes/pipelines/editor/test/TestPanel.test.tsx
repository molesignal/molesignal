import '@/i18n';

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { AxiosError, type AxiosResponse } from 'axios';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as functionsApi from '@/api/functions';
import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';
import { useOrgStore } from '@/stores/useOrgStore';

import { recentEvent } from './recentEvent';
import { sampleEvent } from './sample';
import { TestPanel } from './TestPanel';
import { defaultPipelineGraph, type PipelineGraphModel } from '../../PipelineGraph';

const access = vi.hoisted(() => ({ disabled: false }));

vi.mock('@/api/functions', () => ({ run: vi.fn() }));
vi.mock('./recentEvent', () => ({ recentEvent: vi.fn() }));
vi.mock('@/shell/ui/select', () => import('../../__tests__/nativeSelect'));
vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({
    allowed: !access.disabled,
    disabled: access.disabled,
    reason: access.disabled ? 'Requires functions.run' : undefined,
  }),
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

const run = vi.mocked(functionsApi.run);
const recent = vi.mocked(recentEvent);

function apiFailure(status: number, message: string): AxiosError {
  return new AxiosError(
    `Request failed with status code ${status}`,
    AxiosError.ERR_BAD_REQUEST,
    undefined,
    undefined,
    { status, data: { message } } as AxiosResponse,
  );
}

const ROUTE = {
  name: 'by-app',
  kind: 'builtin' as const,
  operation: 'route' as const,
  script: '',
  target: 'default',
  routing: { kind: 'field' as const, field: 'appname', prefix: 'app_', fallback: 'default', retain_source: false },
};

const MODEL: PipelineGraphModel = {
  ...defaultPipelineGraph('logs'),
  sources: ['app_logs'],
  sinks: ['clean_logs', 'connector:c1'],
  transforms: [
    { name: 'parse', script: 'one' },
    { name: 'tidy', script: 'two' },
  ],
};

function renderPanel(model: PipelineGraphModel = MODEL) {
  const tree = (next: PipelineGraphModel) => (
    <TooltipProvider delayDuration={0}>
      <TestPanel model={next} connectorNames={{ c1: 'Archive' }} />
    </TooltipProvider>
  );
  const view = render(tree(model));

  return { change: (next: PipelineGraphModel) => view.rerender(tree(next)) };
}

const sample = () => screen.getByRole<HTMLTextAreaElement>('textbox', { name: 'Sample event' });
const runButton = () => screen.getByRole('button', { name: 'Run test' });
const result = () => document.querySelector<HTMLElement>('[data-pipeline-test-result]')!;
const shown = () => JSON.parse(document.querySelector('[data-pipeline-test-output]')!.textContent ?? '') as unknown;
const steps = () =>
  Array.from(document.querySelectorAll('[data-test-step]'), (step) => [
    step.getAttribute('data-test-step'),
    step.textContent,
  ]);

async function finished() {
  await waitFor(() => expect(result()).not.toBeNull());
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  access.disabled = false;
  useOrgStore.setState({ currentOrgId: 'org-1' });
  run.mockReset();
  recent.mockReset();
});

afterEach(cleanup);

describe('TestPanel', () => {
  it('takes the sample through every step and shows what would be written, and where', async () => {
    run
      .mockResolvedValueOnce({ output: { level: 'warn' } })
      .mockResolvedValueOnce({ output: { level: 'warn', tidy: true } });
    renderPanel();

    expect(sample().value).toBe(sampleEvent('logs'));
    fireEvent.click(runButton());
    await finished();

    // Each step sees the stream the sample stands for.
    const metadata = { source_stream: 'app_logs' };
    expect(run.mock.calls.map(([request]) => request)).toEqual([
      { language: 'vrl', source: 'one', input: JSON.parse(sampleEvent('logs')), metadata },
      { language: 'vrl', source: 'two', input: { level: 'warn' }, metadata },
    ]);
    expect(result().getAttribute('data-pipeline-test-result')).toBe('passed');
    expect(steps()).toEqual([
      ['passed', '1parse'],
      ['passed', '2tidy'],
    ]);
    expect(shown()).toEqual({ level: 'warn', tidy: true });
    expect(within(result()).getByText('Result')).not.toBeNull();
    // Every destination, the connector by its name.
    expect(result().textContent).toContain('Written to clean_logs, Archive');
  });

  it('shows the event as any earlier step left it', async () => {
    run
      .mockResolvedValueOnce({ output: { step: 1 } })
      .mockResolvedValueOnce({ output: { step: 2 } });
    renderPanel();
    fireEvent.click(runButton());
    await finished();

    expect(within(result()).getByRole('button', { name: /tidy$/ }).getAttribute('aria-pressed')).toBe('true');
    fireEvent.click(within(result()).getByRole('button', { name: /parse$/ }));

    expect(shown()).toEqual({ step: 1 });
    expect(within(result()).getByText('After step 1')).not.toBeNull();
    // Where the event is written is said of the finished event only.
    expect(result().textContent).not.toContain('Written to');
  });

  it('names the stream a routing step sends the event to', async () => {
    run.mockResolvedValueOnce({ output: { appname: 'checkout' } });
    renderPanel({ ...MODEL, sinks: ['default'], transforms: [MODEL.transforms[0]!, ROUTE] });
    fireEvent.click(runButton());
    await finished();

    expect(steps()).toEqual([
      ['passed', '1parse'],
      ['passed', '2by-app → app_checkout'],
    ]);
    expect(result().textContent).toContain('Written to app_checkout');
  });

  it('gives the event back as it went in when the pipeline has no steps', async () => {
    renderPanel({ ...MODEL, transforms: [] });
    fireEvent.click(runButton());
    await finished();

    expect(run).not.toHaveBeenCalled();
    expect(steps()).toEqual([]);
    expect(within(result()).getByText('This pipeline has no steps: the event is written as it is.')).not.toBeNull();
    expect(shown()).toEqual(JSON.parse(sampleEvent('logs')));
    expect(within(result()).getByText('Result')).not.toBeNull();
    expect(result().textContent).toContain('Written to clean_logs, Archive');
  });

  it('writes every stream and delivers to every connector', async () => {
    run.mockResolvedValue({ output: {} });
    renderPanel({ ...MODEL, sinks: ['clean_logs', 'archive_logs', 'connector:c1'] });
    fireEvent.click(runButton());
    await finished();

    expect(result().textContent).toContain('Written to clean_logs, archive_logs, Archive');
  });

  it('says which step dropped the event, and that it goes nowhere', async () => {
    run
      .mockResolvedValueOnce({ output: { level: 'debug' } })
      .mockResolvedValueOnce({ output: { level: 'debug' }, dropped: true });
    renderPanel({ ...MODEL, transforms: [...MODEL.transforms, { name: 'after', script: 'three' }] });
    fireEvent.click(runButton());
    await finished();

    expect(result().getAttribute('data-pipeline-test-result')).toBe('dropped');
    expect(steps()).toEqual([
      ['passed', '1parse'],
      ['dropped', '2tidy · dropped'],
      ['skipped', '3after · not run'],
    ]);
    expect(run).toHaveBeenCalledTimes(2);
    // The event as it came to the step that dropped it.
    expect(shown()).toEqual({ level: 'debug' });
    expect(within(result()).getByText('Dropped by step 2')).not.toBeNull();
    expect(result().textContent).not.toContain('Written to');
    expect(result().textContent).toContain('counts it as dropped, not as failed');
  });

  it('runs the sample as read from the source it is said to come from', async () => {
    run.mockResolvedValue({ output: {} });
    recent.mockResolvedValueOnce({ host: 'web-1' });
    renderPanel({ ...MODEL, sources: ['app_logs', 'nginx'] });

    fireEvent.change(screen.getByRole('combobox', { name: 'Sample from' }), { target: { value: 'nginx' } });
    fireEvent.click(screen.getByRole('button', { name: 'Use a recent event from nginx' }));
    await waitFor(() => expect(JSON.parse(sample().value)).toEqual({ host: 'web-1' }));
    fireEvent.click(runButton());
    await finished();

    expect(recent).toHaveBeenCalledWith('org-1', 'nginx', 'logs');
    expect(run.mock.calls[0]?.[0]).toMatchObject({ metadata: { source_stream: 'nginx' } });
  });

  it('asks where the sample comes from only when there is more than one source', () => {
    renderPanel();

    expect(screen.queryByRole('combobox', { name: 'Sample from' })).toBeNull();
  });

  it('stops at a script that is not valid VRL and says what is wrong with it', async () => {
    run.mockRejectedValueOnce(
      apiFailure(
        400,
        'invalid argument: vrl compile: DiagnosticList([Diagnostic { severity: Error, code: 203, message: "syntax error", labels: [Label { message: "unexpected end of program", primary: true, span: Span { start: 3, end: 4 } }], notes: [] }])',
      ),
    );
    renderPanel();
    fireEvent.click(runButton());
    await finished();

    expect(result().getAttribute('data-pipeline-test-result')).toBe('failed');
    expect(steps()).toEqual([
      ['failed', '1parseNot valid VRL: syntax error: unexpected end of program'],
      ['skipped', '2tidy · not run'],
    ]);
    expect(run).toHaveBeenCalledTimes(1);
    expect(document.querySelector('[data-pipeline-test-output]')).toBeNull();
    expect(result().textContent).toContain('an event that fails a step counts as failed');
  });

  it('keeps what the steps before a failure made of the event', async () => {
    run
      .mockResolvedValueOnce({ output: { step: 1 } })
      .mockRejectedValueOnce(apiFailure(500, 'internal error'));
    renderPanel();
    fireEvent.click(runButton());
    await finished();

    expect(steps()).toEqual([
      ['passed', '1parse'],
      ['failed', '2tidyFailed on this event.'],
    ]);
    expect(shown()).toEqual({ step: 1 });
    expect(within(result()).getByText('After step 1')).not.toBeNull();
    expect(result().textContent).not.toContain('Written to');
  });

  it('says a result is out of date once the steps change', async () => {
    run.mockResolvedValue({ output: {} });
    const { change } = renderPanel();
    fireEvent.click(runButton());
    await finished();
    expect(result().getAttribute('data-stale')).toBeNull();

    // Where the event goes is not what was tested.
    change({ ...MODEL, sinks: ['elsewhere'] });
    expect(result().getAttribute('data-stale')).toBeNull();

    change({ ...MODEL, transforms: [{ name: 'parse', script: 'changed' }, MODEL.transforms[1]!] });
    expect(result().getAttribute('data-stale')).toBe('true');
    expect(within(result()).getByText('The steps changed since this run.')).not.toBeNull();

    fireEvent.click(runButton());
    await waitFor(() => expect(result().getAttribute('data-stale')).toBeNull());
  });

  it('will not run a sample that is not JSON', () => {
    renderPanel();

    fireEvent.change(sample(), { target: { value: '{"level": ' } });

    expect(screen.getByRole('alert').textContent).toBe('The sample is not valid JSON.');
    expect(runButton()).toBeDisabled();

    fireEvent.click(screen.getByRole('button', { name: 'Reset' }));
    expect(sample().value).toBe(sampleEvent('logs'));
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Reset' })).toBeNull();
  });

  it('loads an event the source stream really holds', async () => {
    recent.mockResolvedValueOnce({ level: 'ERROR', message: 'disk full' });
    renderPanel();

    fireEvent.click(screen.getByRole('button', { name: 'Use a recent event from app_logs' }));

    await waitFor(() => expect(JSON.parse(sample().value)).toEqual({ level: 'ERROR', message: 'disk full' }));
    expect(recent).toHaveBeenCalledWith('org-1', 'app_logs', 'logs');
    expect(screen.getByRole('button', { name: 'Reset' })).not.toBeNull();
  });

  it('says so when the stream has nothing recent, and keeps the sample', async () => {
    recent.mockResolvedValueOnce(null);
    renderPanel();

    fireEvent.click(screen.getByRole('button', { name: 'Use a recent event from app_logs' }));

    expect((await screen.findByRole('status')).textContent).toBe(
      'No events in app_logs in the last 24 hours.',
    );
    expect(sample().value).toBe(sampleEvent('logs'));
  });

  it('offers no recent event before a source is chosen', () => {
    renderPanel({ ...MODEL, sources: [''] });

    expect(screen.queryByRole('button', { name: /Use a recent event/ })).toBeNull();
  });

  it('follows the signal with its sample until someone edits it', () => {
    const { change } = renderPanel();

    change({ ...MODEL, signalType: 'metrics' });
    expect(sample().value).toBe(sampleEvent('metrics'));

    fireEvent.change(sample(), { target: { value: '{"mine": true}' } });
    change({ ...MODEL, signalType: 'traces' });
    expect(sample().value).toBe('{"mine": true}');
  });

  it('cannot be run without permission to run functions', () => {
    access.disabled = true;
    renderPanel();

    expect(runButton()).toBeDisabled();
  });
});
