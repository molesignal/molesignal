import '@/i18n';

import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { PipelineForm } from '../PipelineForm';
import type { PipelineGraphModel } from '../PipelineGraph/model';
import { ProcessingEditor } from '../PipelineGraph/ProcessingEditor';
import { SourceRetention } from '../PipelineGraph/SourceRetention';

vi.mock('../PipelineGraph', async () => {
  const model = await import('../PipelineGraph/model');
  return { ...model, PipelineGraphEditor: ({ value, onChange }: { value: PipelineGraphModel; onChange: (value: PipelineGraphModel) => void }) => <><SourceRetention source={value.sources[0] ?? 'default'} checked={value.retainSource ?? false} onChange={(retainSource) => onChange({ ...value, retainSource })} /><ProcessingEditor step={value.transforms[0]!} onChange={(patch) => onChange({ ...value, transforms: [{ ...value.transforms[0]!, ...patch }] })} vrlEditor={<div>VRL editor</div>} /></> };
});
afterEach(cleanup);
const t = (key: string) => i18n.t(key, { ns: 'pipelines' });

describe('realtime form', () => {
  it('hides schedule fields and submits dynamic routing with retained originals', async () => {
    const user = userEvent.setup();
    const submit = vi.fn();
    render(<TooltipProvider><PipelineForm formId="pipeline-test" onSubmit={submit} /><button type="submit" form="pipeline-test">Save</button></TooltipProvider>);
    await user.type(screen.getByRole('textbox', { name: t('flows.form.name_label') }), 'route logs');
    await user.click(screen.getByRole('button', { name: t('realtime.title') }));
    expect(screen.queryByText(t('flows.form.cron_label'))).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: t('processing.builtin') }));
    expect(screen.queryByText('VRL editor')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: t('realtime.field') }));
    await user.click(screen.getByRole('checkbox', { name: t('realtime.retain') }));
    await user.click(screen.getByRole('button', { name: 'Save' }));
    expect(submit).toHaveBeenCalledWith(expect.objectContaining({ cron: '', function_steps: expect.objectContaining({ mode: 'realtime', retain_source: true, steps: [expect.objectContaining({ kind: 'builtin', operation: 'route', routing: expect.objectContaining({ kind: 'field', field: 'appname' }) })] }) }));
    await user.click(screen.getByRole('button', { name: t('realtime.scheduled') }));
    expect(screen.getByText(t('flows.form.cron_label'))).toBeVisible();
    await user.click(screen.getByRole('button', { name: t('processing.vrl') }));
    expect(screen.getByText('VRL editor')).toBeVisible();
  });
});
