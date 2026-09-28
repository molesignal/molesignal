import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { PipelineForm } from '../PipelineForm';

vi.mock('@/api/connectors', () => ({ list: async () => [] }));
vi.mock('@/api/functions', () => ({ list: async () => [] }));
vi.mock('@/api/streams', () => ({ list: async () => [] }));
afterEach(cleanup);

it('switches processing types through the actual workbench and saves built-in routing', async () => {
  const user = userEvent.setup();
  const submit = vi.fn();
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const t = (key: string) => i18n.t(key, { ns: 'pipelines' });
  render(<QueryClientProvider client={query}><TooltipProvider>
    <PipelineForm formId="processing-workbench" onSubmit={submit} />
    <button type="submit" form="processing-workbench">Save</button>
  </TooltipProvider></QueryClientProvider>);
  await user.type(screen.getByRole('textbox', { name: t('flows.form.name_label') }), 'route-apps');
  await user.click(screen.getByRole('button', { name: t('realtime.title') }));
  expect(screen.queryByText(t('flows.form.cron_label'))).not.toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: t('processing.builtin') }));
  expect(screen.queryByText(t('graph.vrl_script'), { exact: true })).not.toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: t('realtime.field') }));
  await user.clear(screen.getByRole('textbox', { name: new RegExp(t('realtime.field_name')) }));
  await user.type(screen.getByRole('textbox', { name: new RegExp(t('realtime.field_name')) }), 'appname');
  await user.click(screen.getByRole('button', { name: 'Save' }));
  expect(submit).toHaveBeenCalledWith(expect.objectContaining({ function_steps: expect.objectContaining({
    mode: 'realtime', steps: [expect.objectContaining({ kind: 'builtin', operation: 'route', routing: expect.objectContaining({ field: 'appname', kind: 'field' }) })],
  }) }));
  await user.click(screen.getByRole('button', { name: t('processing.vrl') }));
  expect(screen.getByText(t('graph.vrl_script'), { exact: true })).toBeVisible();
  query.clear();
});
