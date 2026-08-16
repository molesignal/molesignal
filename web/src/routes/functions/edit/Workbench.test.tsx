import { render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';

import { FunctionWorkbench } from './Workbench';

vi.mock('@/shell/codeEditor', () => ({
  CodeEditor: () => <div data-code-editor />,
}));

const saveFunctionLabel = i18n.t('functions:edit.save');

describe('FunctionWorkbench cardless layout', () => {
  it('uses flat definition and editor bands without card containers', () => {
    const { container } = render(
      <FunctionWorkbench
        name="Normalize logs"
        language="vrl"
        source={'.service = "api"'}
        sampleInput="{}"
        sampleOutput="{}"
        validation={{ kind: 'pending' }}
        runState={{ kind: 'idle' }}
        validationMessage="Pending"
        outputPlaceholder="Run the sample"
        sampleInputError={null}
        writeAccess={{ allowed: true, disabled: false }}
        runAccess={{ allowed: true, disabled: false }}
        savePending={false}
        runPending={false}
        canRun
        canSave
        actions={<button type="button">{saveFunctionLabel}</button>}
        onSubmit={vi.fn()}
        onNameChange={vi.fn()}
        onLanguageChange={vi.fn()}
        onSourceChange={vi.fn()}
        onSampleInputChange={vi.fn()}
        onFormatSource={vi.fn()}
        onFormatInput={vi.fn()}
        onRun={vi.fn()}
        onSave={vi.fn()}
      />,
    );

    const definition = container.querySelector('[data-function-definition]');
    expect(definition?.className).not.toMatch(/rounded|shadow|bg-bg-1/);

    const workbench = container.querySelector('[data-function-workbench]');
    expect(workbench?.className).toContain('divide-y');
    expect(workbench?.className).toContain('border-y');
    expect(workbench?.className).not.toContain('flex-1');
    expect(workbench?.className).not.toMatch(/rounded|shadow|bg-bg-1/);

    const actions = container.querySelector('[data-function-actions]');
    expect(actions).toHaveTextContent(saveFunctionLabel);
    expect(actions?.className).toContain('mt-auto');
    expect(actions?.className).toContain('sm:pb-10');
    expect(actions?.className).toContain('justify-end');
  });

  it('keeps detailed validation errors beside the editor instead of in a bottom footer', () => {
    const { container } = render(
      <FunctionWorkbench
        name="Normalize logs"
        language="js"
        source={'molesignal.set("service", "api");'}
        sampleInput="{}"
        sampleOutput=""
        validation={{ kind: 'invalid', message: 'Invalid source' }}
        runState={{ kind: 'idle' }}
        validationMessage="Invalid source"
        outputPlaceholder="Run the sample"
        sampleInputError={null}
        writeAccess={{ allowed: true, disabled: false }}
        runAccess={{ allowed: true, disabled: false }}
        savePending={false}
        runPending={false}
        canRun
        canSave
        actions={<button type="button">{saveFunctionLabel}</button>}
        onSubmit={vi.fn()}
        onNameChange={vi.fn()}
        onLanguageChange={vi.fn()}
        onSourceChange={vi.fn()}
        onSampleInputChange={vi.fn()}
        onFormatSource={vi.fn()}
        onFormatInput={vi.fn()}
        onRun={vi.fn()}
        onSave={vi.fn()}
      />,
    );

    const detail = container.querySelector('[data-function-validation-detail]');
    expect(detail).toHaveTextContent('Invalid source');
    expect(detail?.className).not.toContain('mt-auto');
  });
});
