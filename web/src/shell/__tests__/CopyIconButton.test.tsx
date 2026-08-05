import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ComponentProps } from 'react';
import { describe, expect, it, vi } from 'vitest';

import { CopyIconButton } from '@/shell/CopyIconButton';
import { TooltipProvider } from '@/shell/ui/tooltip';

function renderCopyButton(
  props: ComponentProps<typeof CopyIconButton>,
) {
  return render(
    <TooltipProvider delayDuration={0}>
      <CopyIconButton {...props} />
    </TooltipProvider>,
  );
}

describe('CopyIconButton', () => {
  it('renders one icon without visible button text and exposes the label', async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    renderCopyButton({ label: 'Copy workspace ID', onClick });

    const button = screen.getByRole('button', {
      name: 'Copy workspace ID',
    });
    expect(button.textContent).toBe('');
    expect(button.querySelectorAll('svg')).toHaveLength(1);
    expect(within(button).queryByText('Copy workspace ID')).toBeNull();

    await user.hover(button);
    expect((await screen.findByRole('tooltip')).textContent).toBe(
      'Copy workspace ID',
    );
    await user.click(button);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('keeps the success state to one icon', () => {
    renderCopyButton({
      label: 'Copy workspace ID',
      copied: true,
      copiedLabel: 'Workspace ID copied',
    });

    const button = screen.getByRole('button', {
      name: 'Workspace ID copied',
    });
    expect(button.textContent).toBe('');
    expect(button.querySelectorAll('svg')).toHaveLength(1);
  });
});
