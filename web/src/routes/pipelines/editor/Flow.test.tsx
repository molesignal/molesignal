import '@/i18n';

import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { PipelineFlow, type FlowEditing } from './Flow';
import { defaultPipelineGraph, type PipelineGraphModel } from '../PipelineGraph';

const MODEL: PipelineGraphModel = {
  ...defaultPipelineGraph('logs'),
  sources: ['app_logs'],
  sinks: ['clean_logs', 'connector:c1'],
  transforms: [
    { name: 'parse', script: '.', processor: { op: 'rename', from: 'msg', to: 'message' } },
    { name: '', script: '. = .' },
  ],
};

function editing(overrides: Partial<FlowEditing> = {}): FlowEditing {
  return {
    selectedId: 'transform-0',
    onSelect: vi.fn(),
    onAddStep: vi.fn(),
    onAddSource: vi.fn(),
    onAddDestination: vi.fn(),
    issues: [],
    ...overrides,
  };
}

function renderFlow(model: PipelineGraphModel, state?: FlowEditing) {
  render(
    <TooltipProvider delayDuration={0}>
      <PipelineFlow model={model} connectorNames={{ c1: 'Archive' }} editing={state} />
    </TooltipProvider>,
  );
}

const card = (id: string) => document.querySelector<HTMLElement>(`[data-pipeline-node="${id}"]`)!;
const lane = (name: string) => within(screen.getByRole('group', { name }));
const mark = (id: string) =>
  card(id).querySelector('[data-flow-node-issue]')?.getAttribute('data-flow-node-issue') ?? null;

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  Element.prototype.scrollIntoView = vi.fn();
});

afterEach(cleanup);

describe('PipelineFlow', () => {
  it('draws a saved pipeline as a picture: nothing in it can be pressed', () => {
    renderFlow(MODEL);

    expect(screen.queryAllByRole('button')).toEqual([]);
    expect(card('source-0').tagName).toBe('DIV');
    expect(lane('Source').getByText('app_logs')).not.toBeNull();
    expect(lane('Processing').getByText('Rename field · msg → message')).not.toBeNull();
    expect(lane('Destinations').getByText('clean_logs')).not.toBeNull();
    // A connector goes by its name, not its id.
    expect(lane('Destinations').getByText('Archive')).not.toBeNull();
    expect(lane('Destinations').getByText('Connector delivery')).not.toBeNull();
  });

  it('prompts for what a node has no name for yet', () => {
    renderFlow({ ...MODEL, sources: [''], sinks: ['', 'connector:'] });

    expect(card('source-0').textContent).toContain('Choose a stream');
    expect(card('transform-1').textContent).toContain('Unnamed step');
    expect(card('sink-0').textContent).toContain('Choose a stream');
    expect(card('sink-1').textContent).toContain('Choose a connector');
  });

  it('draws every source it reads and every stream it writes', () => {
    renderFlow({
      ...MODEL,
      sources: ['app_logs', 'audit_logs'],
      sinks: ['clean_logs', 'connector:c1', 'archive_logs'],
    });

    expect(card('source-0').textContent).toBe('app_logsStream input');
    expect(card('source-1').textContent).toBe('audit_logsStream input');
    expect(card('sink-0').textContent).toBe('clean_logsStream output');
    expect(card('sink-2').textContent).toBe('archive_logsStream output');
    expect(card('sink-1').textContent).toBe('ArchiveConnector delivery');
  });

  it('says so when there are no steps', () => {
    renderFlow({ ...MODEL, transforms: [] });

    expect(lane('Processing').getByText('No steps: events are written as they are read.')).not.toBeNull();
    expect(document.querySelectorAll('[data-pipeline-node^="transform-"]')).toHaveLength(0);
  });

  it('lets the routing step stand for the first stream; the others take every event', () => {
    renderFlow({
      ...MODEL,
      sinks: ['default', 'archive_logs'],
      transforms: [
        {
          name: 'route',
          kind: 'builtin',
          operation: 'route',
          script: '',
          target: 'default',
          routing: { kind: 'field', field: 'appname', prefix: '', fallback: 'default', retain_source: false },
        },
      ],
    });

    expect(card('sink-0').textContent).toBe('Stream outputChosen by the routing step');
    expect(card('sink-1').textContent).toBe('archive_logsTakes every event');
    expect(lane('Processing').getByText('Route to stream')).not.toBeNull();
  });

  it('says whether a filter keeps or drops what it matches', () => {
    renderFlow({
      ...MODEL,
      transforms: [
        {
          name: 'quiet',
          script: '.',
          processor: { op: 'filter', action: 'drop', field: 'level', test: 'equals', value: 'debug' },
        },
      ],
    });

    expect(lane('Processing').getByText('Filter events · drop level = debug')).not.toBeNull();
  });

  it('picks a node to edit and marks the one that is picked', () => {
    const state = editing();
    renderFlow(MODEL, state);

    expect(card('transform-0').getAttribute('aria-pressed')).toBe('true');
    expect(card('sink-1').getAttribute('aria-pressed')).toBe('false');
    fireEvent.click(card('sink-1'));

    expect(state.onSelect).toHaveBeenCalledWith('sink-1');
    // The picked card is brought into the lane's view.
    expect(Element.prototype.scrollIntoView).toHaveBeenCalledTimes(1);
  });

  it('marks what is wrong, what is left to do and what is only worth knowing, each its own way', () => {
    renderFlow(
      { ...MODEL, sources: [''] },
      editing({
        issues: [
          { level: 'error', code: 'source_missing', node: 'source-0', pending: true },
          { level: 'error', code: 'processor_number_invalid', node: 'transform-0', params: { name: 'parse' } },
          { level: 'warning', code: 'transform_name_missing', node: 'transform-1', params: { index: 2 } },
          { level: 'error', code: 'transform_script_missing', node: 'transform-1', pending: true },
        ],
      }),
    );

    // Not filled in yet: a quiet mark, and not an invalid field.
    expect(mark('source-0')).toBe('pending');
    expect(card('source-0').getAttribute('aria-invalid')).toBeNull();
    expect(card('source-0').getAttribute('aria-label')).toBe('Choose a stream, 1 issue');
    // Written, and wrong.
    expect(mark('transform-0')).toBe('error');
    expect(card('transform-0').getAttribute('aria-invalid')).toBe('true');
    expect(mark('transform-1')).toBe('pending');
    expect(card('transform-1').getAttribute('aria-label')).toBe('Unnamed step, 2 issues');
    expect(mark('sink-0')).toBeNull();
    expect(card('sink-0').getAttribute('aria-label')).toBeNull();
  });

  it('turns what is left to do into errors once a save was held back', () => {
    renderFlow(
      { ...MODEL, sources: [''] },
      editing({
        urgent: true,
        issues: [
          { level: 'error', code: 'source_missing', node: 'source-0', pending: true },
          { level: 'warning', code: 'connector_missing', node: 'sink-1', params: { id: 'c1' } },
        ],
      }),
    );

    expect(mark('source-0')).toBe('error');
    expect(card('source-0').getAttribute('aria-invalid')).toBe('true');
    expect(mark('sink-1')).toBe('warning');
    expect(card('sink-1').getAttribute('aria-invalid')).toBeNull();
  });

  it('adds a source, a step of a chosen kind, or a destination of a chosen kind', async () => {
    const user = userEvent.setup();
    const state = editing();
    renderFlow(MODEL, state);

    fireEvent.click(screen.getByRole('button', { name: 'Add source' }));
    expect(state.onAddSource).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole('button', { name: 'Add destination' }));
    expect((await screen.findAllByRole('menuitem')).map((item) => item.textContent)).toEqual([
      'Stream',
      'Connector',
    ]);
    await user.click(screen.getByRole('menuitem', { name: 'Connector' }));
    expect(state.onAddDestination).toHaveBeenCalledWith('connector');

    await user.click(screen.getByRole('button', { name: 'Add step' }));
    expect((await screen.findAllByRole('menuitem')).map((item) => item.textContent)).toEqual([
      'Parse JSON',
      'Parse key=value',
      'Extract with regex',
      'Rename field',
      'Remove fields',
      'Set field',
      'Mask field',
      'Change case',
      'Convert type',
      'Filter events',
      'Sample',
      'Route to stream',
      'Custom VRL',
    ]);
    await user.click(screen.getByRole('menuitem', { name: 'Filter events' }));

    expect(state.onAddStep).toHaveBeenCalledWith('filter');
  });

  it('adds no more streams to a side that has as many as a pipeline takes', async () => {
    const user = userEvent.setup();
    const many = Array.from({ length: 8 }, (_, index) => `stream_${index}`);
    renderFlow({ ...MODEL, sources: many, sinks: many.map((name) => `${name}_out`) }, editing());

    expect(screen.getByRole('button', { name: 'Add source' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Add destination' }));
    expect(await screen.findByRole('menuitem', { name: 'Stream' })).toHaveAttribute('data-disabled');
  });

  it('reads one stream and writes to its stream and nowhere else in realtime mode', () => {
    renderFlow({ ...MODEL, mode: 'realtime', sinks: ['clean_logs'] }, editing());

    expect(screen.queryByRole('button', { name: 'Add source' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Add destination' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Add step' })).not.toBeNull();
  });

  it('adds nothing while the editor is read-only', () => {
    renderFlow(MODEL, editing({ disabled: true, disabledReason: 'Requires pipelines.update' }));

    expect(screen.getByRole('button', { name: 'Add step' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Add source' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Add destination' })).toBeDisabled();
  });
});
