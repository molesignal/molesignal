import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';

import { DataTable } from '@/admin';
import '@/i18n';
import { ChromeButton } from '@/shell/chrome';
import { FilePicker } from '@/shell/FilePicker';
import {
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { Button } from '@/shell/ui/button';
import { TooltipProvider } from '@/shell/ui/tooltip';

function withTooltips(node: ReactNode) {
  return render(
    <TooltipProvider delayDuration={0}>{node}</TooltipProvider>,
  );
}

describe('disabled action foundations', () => {
  it('suppresses ChromeButton activation and exposes its reason', async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    withTooltips(
      <ChromeButton
        disabled
        disabledReason="Requires dashboards.create"
        onClick={onClick}
      >
        Create
      </ChromeButton>,
    );

    const button = screen.getByRole('button', { name: 'Create' });
    expect((button as HTMLButtonElement).disabled).toBe(true);
    expect(button.getAttribute('aria-disabled')).toBe('true');
    await user.click(button);
    expect(onClick).not.toHaveBeenCalled();

    await user.hover(
      button.closest('[data-disabled-control]') as HTMLElement,
    );
    expect(
      await screen.findAllByText('Requires dashboards.create'),
    ).not.toHaveLength(0);
  });

  it('renders long disabled reasons outside clipping containers', async () => {
    const user = userEvent.setup();
    const { container } = withTooltips(
      <div data-testid="clipping-container" className="overflow-hidden whitespace-nowrap">
        <ChromeButton
          disabled
          disabledReason="At least one tenant organization must remain enabled."
        >
          Disable
        </ChromeButton>
      </div>,
    );

    await user.hover(
      container.querySelector('[data-disabled-control]') as HTMLElement,
    );
    const tooltipText = (
      await screen.findAllByText(
        'At least one tenant organization must remain enabled.',
      )
    ).at(-1) as HTMLElement;
    const tooltip = tooltipText.closest('[role="tooltip"]') as HTMLElement;
    expect(tooltip).not.toBeNull();
    expect(
      screen.getByTestId('clipping-container').contains(tooltip),
    ).toBe(false);
    expect(tooltip.className).toContain('whitespace-normal');
    expect(tooltip.className).toContain('break-words');
  });

  it('prevents disabled asChild links from mouse activation', async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    withTooltips(
      <Button
        asChild
        disabled
        disabledReason="Requires intelligence.manage"
      >
        <a href="/write" onClick={onClick}>
          Edit
        </a>
      </Button>,
    );

    const link = screen.getByRole('link', { name: 'Edit' });
    expect(link.getAttribute('aria-disabled')).toBe('true');
    expect(link.getAttribute('tabindex')).toBe('-1');
    await user.click(link);
    expect(onClick).not.toHaveBeenCalled();
  });

  it('suppresses mouse and keyboard activation for disabled table rows', () => {
    const onRowClick = vi.fn();
    withTooltips(
      <DataTable
        rows={[{ id: 'locked', name: 'Locked row' }]}
        rowKey={(row) => row.id}
        columns={[
          {
            key: 'name',
            header: 'Name',
            cell: (row) => row.name,
          },
        ]}
        onRowClick={onRowClick}
        isRowClickDisabled={() => true}
        rowClickDisabledReason={() => 'Read-only'}
      />,
    );

    const row = screen.getByRole('row', { name: 'Locked row' });
    expect(row.getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(row);
    fireEvent.keyDown(row, { key: 'Enter' });
    fireEvent.keyDown(row, { key: ' ' });
    expect(onRowClick).not.toHaveBeenCalled();
  });

  it('keeps disabled textareas read-only to mouse and keyboard users', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    withTooltips(
      <FormTextarea
        aria-label="Policy"
        disabled
        disabledReason="Requires org.settings.manage"
        value="read-only policy"
        onChange={onChange}
      />,
    );

    const textarea = screen.getByRole('textbox', { name: 'Policy' });
    expect((textarea as HTMLTextAreaElement).disabled).toBe(true);
    expect(textarea.getAttribute('aria-disabled')).toBe('true');
    await user.click(textarea);
    await user.keyboard('changed');
    expect(onChange).not.toHaveBeenCalled();
  });

  it('does not open or process a disabled file picker', async () => {
    const onFile = vi.fn();
    const { container } = withTooltips(
      <FilePicker
        buttonLabel="Upload license"
        disabled
        disabledReason="Requires sys.licenses.manage"
        onFile={onFile}
      />,
    );

    const input = container.querySelector('input[type="file"]');
    expect(input).not.toBeNull();
    expect((input as HTMLInputElement).disabled).toBe(true);
    expect(input?.getAttribute('aria-disabled')).toBe('true');
    fireEvent.change(input as HTMLInputElement, {
      target: { files: [new File(['license'], 'license.txt')] },
    });
    expect(onFile).not.toHaveBeenCalled();
  });

  it('explains invalid submit states by default', async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    withTooltips(
      <FormSubmitFooter invalid onCancel={onCancel} submitLabel="Save" />,
    );

    const submit = screen.getByRole('button', { name: 'Save' });
    expect((submit as HTMLButtonElement).disabled).toBe(true);
    expect(submit.getAttribute('aria-disabled')).toBe('true');
    await user.hover(
      submit.closest('[data-disabled-control]') as HTMLElement,
    );
    expect(
      await screen.findAllByText(
        'Complete the required fields before saving.',
      ),
    ).not.toHaveLength(0);
  });
});
