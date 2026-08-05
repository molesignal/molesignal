import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import * as React from 'react';
import { afterEach, describe, expect, it } from 'vitest';

import { CollapsibleSidePanel, SidePanelSection } from '@/shell/CollapsibleSidePanel';

afterEach(() => {
  cleanup();
});

function firePointerEvent(
  target: Window | Document | Node,
  type: 'pointerdown' | 'pointermove' | 'pointerup',
  options: { pointerId: number; clientX: number },
): void {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    button: 0,
    clientX: options.clientX,
  });
  Object.defineProperty(event, 'pointerId', {
    configurable: true,
    value: options.pointerId,
  });
  fireEvent(target, event);
}

function UtilityPanelHarness() {
  const [collapsed, setCollapsed] = React.useState(false);

  return (
    <CollapsibleSidePanel
      title="Fields"
      collapsed={collapsed}
      onCollapsedChange={setCollapsed}
      variant="utility"
      widthClassName="w-[240px]"
      resizable
      defaultWidth={240}
      resizeLabel="Resize fields"
      collapseLabel="Collapse fields"
      expandLabel="Expand fields"
    >
      <SidePanelSection title="Common fields" count={2}>
        <div>service</div>
        <div>level</div>
      </SidePanelSection>
    </CollapsibleSidePanel>
  );
}

describe('CollapsibleSidePanel', () => {
  it('renders the lightweight utility treatment and its field grouping', () => {
    render(<UtilityPanelHarness />);

    const panel = screen.getByText('Fields').closest('aside');
    expect(panel?.getAttribute('data-variant')).toBe('utility');
    expect(panel?.className).toContain('w-[240px]');
    expect(screen.getByText('Common fields')).not.toBeNull();
    expect(screen.getByText('2')).not.toBeNull();
  });

  it('collapses to a narrow rail and can be expanded again', () => {
    render(<UtilityPanelHarness />);

    fireEvent.click(screen.getByRole('button', { name: 'Collapse fields' }));

    const expandButton = screen.getByRole('button', { name: 'Expand fields' });
    expect(expandButton).not.toBeNull();
    expect(screen.queryByText('Common fields')).toBeNull();

    fireEvent.click(expandButton);

    expect(screen.getByRole('button', { name: 'Collapse fields' })).not.toBeNull();
    expect(screen.getByText('Common fields')).not.toBeNull();
  });

  it('resizes by pointer and keyboard within the pixel bounds', () => {
    render(<UtilityPanelHarness />);

    const panel = screen.getByText('Fields').closest('aside');
    const separator = screen.getByRole('separator', { name: 'Resize fields' });

    expect(panel?.style.getPropertyValue('--side-panel-width')).toBe('240px');
    expect(separator.getAttribute('aria-valuemin')).toBe('240');
    expect(separator.getAttribute('aria-valuemax')).toBe('480');

    firePointerEvent(separator, 'pointerdown', { pointerId: 1, clientX: 240 });
    firePointerEvent(window, 'pointermove', { pointerId: 1, clientX: 900 });
    firePointerEvent(window, 'pointerup', { pointerId: 1, clientX: 900 });

    expect(panel?.style.getPropertyValue('--side-panel-width')).toBe('480px');
    expect(separator.getAttribute('aria-valuenow')).toBe('480');

    fireEvent.keyDown(separator, { key: 'Home' });
    expect(panel?.style.getPropertyValue('--side-panel-width')).toBe('240px');

    fireEvent.keyDown(separator, { key: 'ArrowRight', shiftKey: true });
    expect(panel?.style.getPropertyValue('--side-panel-width')).toBe('272px');

    fireEvent.doubleClick(separator);
    expect(panel?.style.getPropertyValue('--side-panel-width')).toBe('240px');
  });
});
