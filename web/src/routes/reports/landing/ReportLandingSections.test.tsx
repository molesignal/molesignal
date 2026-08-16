import '@/i18n';

import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { TemplatePreset } from '../reportTypes';
import { ReportStarter, TemplateLibrary } from './ReportLandingSections';

vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({
    allowed: true,
    disabled: false,
    reason: undefined,
  }),
}));

const builtInTemplate: TemplatePreset = {
  id: 'builtin:platform-health',
  serverId: null,
  isBuiltin: true,
  name: 'Platform health weekly',
  description: 'Availability, latency, errors, and capacity.',
  sourceKind: 'dashboard',
  format: 'pdf',
  rangePreset: 'previous-calendar-week',
  icon: 'platform',
};

const customTemplate: TemplatePreset = {
  ...builtInTemplate,
  id: 'remote:custom',
  serverId: 'custom',
  isBuiltin: false,
  name: 'Customer review',
};

describe('cardless report landing sections', () => {
  it('renders starter choices as flat band cells and preserves their actions', () => {
    const onUseTemplate = vi.fn();
    const onCustom = vi.fn();
    render(
      <ReportStarter
        templates={[builtInTemplate]}
        onUseTemplate={onUseTemplate}
        onCustom={onCustom}
      />,
    );

    const templateButton = screen.getByRole('button', {
      name: /Platform health weekly/i,
    });
    expect(templateButton.className).not.toMatch(/rounded|border|shadow/);
    fireEvent.click(templateButton);
    fireEvent.click(screen.getByRole('button', { name: /Custom report/i }));

    expect(onUseTemplate).toHaveBeenCalledWith(builtInTemplate);
    expect(onCustom).toHaveBeenCalledOnce();
  });

  it('renders templates as divided rows with retry, edit, and use actions', () => {
    const onRetry = vi.fn();
    const onUse = vi.fn();
    const onEdit = vi.fn();
    const { container } = render(
      <TemplateLibrary
        templates={[builtInTemplate, customTemplate]}
        apiError
        onRetry={onRetry}
        onUse={onUse}
        onEdit={onEdit}
      />,
    );

    const rows = Array.from(container.querySelectorAll('article'));
    expect(rows).toHaveLength(2);
    rows.forEach((row) => {
      expect(row.className).not.toMatch(/rounded|border|shadow/);
    });

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));
    fireEvent.click(screen.getAllByRole('button', { name: 'Use template' })[0]!);

    expect(onRetry).toHaveBeenCalledOnce();
    expect(onEdit).toHaveBeenCalledWith(customTemplate);
    expect(onUse).toHaveBeenCalledWith(builtInTemplate);
  });
});
