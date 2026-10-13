import { render, screen } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';

import { FunctionWorkbench } from './Workbench';

vi.mock('@/shell/codeEditor', () => ({
  CodeEditor: () => <div data-code-editor />,
}));

function renderWorkbench(patch: Partial<ComponentProps<typeof FunctionWorkbench>> = {}) {
  return render(
    <FunctionWorkbench
      name="Normalize logs"
      language="vrl"
      source={'.service = "api"'}
      sampleInput="{}"
      sampleOutput="{}"
      validation={{ kind: 'pending' }}
      runState={{ kind: 'idle' }}
      outputPlaceholder="Run the sample"
      sampleInputError={null}
      writeAccess={{ allowed: true, disabled: false }}
      runAccess={{ allowed: true, disabled: false }}
      savePending={false}
      runPending={false}
      canRun
      canSave
      onSubmit={vi.fn()}
      onNameChange={vi.fn()}
      onLanguageChange={vi.fn()}
      onSourceChange={vi.fn()}
      onSampleInputChange={vi.fn()}
      onFormatSource={vi.fn()}
      onFormatInput={vi.fn()}
      onRun={vi.fn()}
      onSave={vi.fn()}
      {...patch}
    />,
  );
}

describe('FunctionWorkbench layout', () => {
  it('stands as cards on the canvas: the definition, the source, the test', () => {
    const { container } = renderWorkbench();

    const cards = [
      container.querySelector('[data-function-definition]'),
      container.querySelector('[data-function-source]'),
      container.querySelector('[data-function-test]'),
    ];
    for (const card of cards) {
      expect(card?.className).toContain('rounded-md');
      expect(card?.className).toContain('bg-[var(--functional-surface)]');
      // A surface, not a ruled box.
      expect(card?.className).not.toMatch(/\bborder(-[yxtblr])?\b(?!-0)/);
    }

    // The source and the test stand side by side, a gap apart, as cards do everywhere.
    const workbench = container.querySelector('[data-function-workbench]');
    expect(workbench?.className).toContain('gap-[12px]');
    expect(workbench?.className).not.toMatch(/divide-|border-y/);
  });

  it('leaves saving to the page: the form is what the title row submits', () => {
    const { container } = renderWorkbench();

    expect(container.querySelector('form')).toHaveAttribute('id', 'function-form');
    expect(container.querySelector('[data-function-actions]')).toBeNull();
    expect(screen.queryByRole('button', { name: i18n.t('functions:edit.save') })).toBeNull();
  });

  it('offers the sample run once, at the head of the test card', () => {
    const { container } = renderWorkbench();

    const runs = container.querySelectorAll('[data-function-run]');
    expect(runs).toHaveLength(1);
    expect(runs[0]?.closest('[data-function-test]')).not.toBeNull();
    expect(runs[0]).toHaveTextContent(i18n.t('functions:edit.run_sample'));
  });

  it('names the language once, where it is chosen', () => {
    const { container } = renderWorkbench();

    // The select shows it; the source card does not repeat it as a tag.
    expect(container.querySelector('[data-function-source]')).not.toHaveTextContent('VRL');
  });

  it('says a state in a mark and a word, with no box around it', () => {
    const { container } = renderWorkbench({ validation: { kind: 'valid' } });

    const status = container.querySelector("[data-function-status='valid']");
    expect(status).toHaveTextContent(i18n.t('functions:edit.validation.valid'));
    expect(status?.className).not.toMatch(/border|rounded-full|bg-/);
  });

  it('writes why a save was refused over the source', () => {
    const { container } = renderWorkbench({
      language: 'js',
      source: 'molesignal.set("service", "api");',
      validation: { kind: 'invalid', message: 'Invalid source' },
    });

    const detail = container.querySelector('[data-function-validation-detail]');
    expect(detail).toHaveTextContent('Invalid source');
    expect(detail?.closest('[data-function-source]')).not.toBeNull();
    expect(detail?.className).toContain('bg-red-dim');
    expect(detail?.className).not.toContain('border');
  });

  it('says the sample input is wrong only when it is', () => {
    const valid = renderWorkbench();
    expect(valid.container.querySelector('[data-function-input-invalid]')).toBeNull();
    valid.unmount();

    const invalid = renderWorkbench({ sampleInputError: 'Sample input must be valid JSON.' });
    expect(invalid.container.querySelector('[data-function-input-invalid]')).toHaveTextContent(
      i18n.t('functions:edit.json_invalid'),
    );
  });

  it('shows why a run failed once, in place of its result', () => {
    const { container } = renderWorkbench({
      validation: { kind: 'invalid' },
      runState: { kind: 'error', durationMs: 12, message: 'function call error for "parse_json"' },
    });

    const error = container.querySelector('[data-function-run-error]');
    expect(error).toHaveTextContent('function call error for "parse_json"');
    expect(container.querySelector("[data-function-test] [data-function-status='invalid']")).not.toBeNull();
    // The source says it is not valid, as a word; the reason is not written a second time.
    expect(container.querySelector("[data-function-source] [data-function-status='invalid']")).not.toBeNull();
    expect(container.querySelector('[data-function-validation-detail]')).toBeNull();
  });
});
