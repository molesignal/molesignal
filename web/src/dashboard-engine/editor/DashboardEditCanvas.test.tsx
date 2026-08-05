import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter } from 'react-router-dom';

import { TooltipProvider } from '@/shell/ui/tooltip';

import { createDashboardPanel, createDashboardText } from '../factories';
import { createEmptyDashboardDefinition } from '../model';
import { DashboardEditCanvas } from './DashboardEditCanvas';

afterEach(cleanup);

describe('DashboardEditCanvas', () => {
  it('edits the production Dashboard canvas instead of a Layout preview', () => {
    const definition = createEmptyDashboardDefinition('Editable dashboard');
    const text = createDashboardText();
    text.id = 'text-live';
    text.title = 'Runbook';
    text.content = 'Rendered dashboard content';
    text.gridPos = { ...text.gridPos, x: 1, y: 0 };
    definition.elements = [text];
    definition.refreshSettings = {
      enabled: false,
      mode: 'off',
      allowedIntervals: ['off'],
    };
    const onSelect = vi.fn();
    const onCommitElements = vi.fn();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });

    const { container } = render(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <TooltipProvider delayDuration={0}>
            <DashboardEditCanvas
              definition={definition}
              orgId="org-1"
              selectedIds={new Set(['text-live'])}
              clipboardSize={0}
              onSelect={onSelect}
              onOpenPanel={vi.fn()}
              onCommitElements={onCommitElements}
              onCopy={vi.fn()}
              onPaste={vi.fn()}
              onDuplicateElement={vi.fn()}
              onRemoveElement={vi.fn()}
              onExport={vi.fn()}
            />
          </TooltipProvider>
        </QueryClientProvider>
      </MemoryRouter>,
    );

    expect(screen.getByText('Editing dashboard')).toBeTruthy();
    expect(screen.getByText('Rendered dashboard content')).toBeTruthy();
    expect(
      screen.queryByRole('button', { name: 'Layout' }),
    ).toBeNull();

    const editItem = container.querySelector<HTMLElement>(
      '[data-dashboard-edit-element="text-live"]',
    );
    fireEvent.click(editItem!);
    expect(onSelect).toHaveBeenCalledWith(new Set(['text-live']));

    fireEvent.keyDown(
      screen.getByLabelText('Dashboard edit canvas'),
      { key: 'ArrowRight' },
    );
    const [nextElements] = onCommitElements.mock.calls.at(-1) ?? [];
    expect(nextElements?.[0]?.gridPos.x).toBe(2);
  });

  it('moves a panel from its surface and opens editing only from Edit', async () => {
    const user = userEvent.setup();
    const definition = createEmptyDashboardDefinition('Direct manipulation');
    const panel = createDashboardPanel([], 'stat');
    panel.id = 'panel-live';
    panel.title = 'Live requests';
    panel.queries = [];
    definition.elements = [panel];
    definition.refreshSettings = {
      enabled: false,
      mode: 'off',
      allowedIntervals: ['off'],
    };
    const onSelect = vi.fn();
    const onOpenPanel = vi.fn();
    const onCommitElements = vi.fn();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });

    const { container } = render(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <TooltipProvider delayDuration={0}>
            <DashboardEditCanvas
              definition={definition}
              orgId=""
              selectedIds={new Set()}
              clipboardSize={0}
              onSelect={onSelect}
              onOpenPanel={onOpenPanel}
              onCommitElements={onCommitElements}
              onCopy={vi.fn()}
              onPaste={vi.fn()}
              onDuplicateElement={vi.fn()}
              onRemoveElement={vi.fn()}
              onExport={vi.fn()}
            />
          </TooltipProvider>
        </QueryClientProvider>
      </MemoryRouter>,
    );

    const grid = container.querySelector<HTMLElement>(
      '[data-dashboard-editor-grid]',
    );
    vi.spyOn(grid!, 'getBoundingClientRect').mockReturnValue({
      width: 1_200,
      height: 600,
      x: 0,
      y: 0,
      top: 0,
      right: 1_200,
      bottom: 600,
      left: 0,
      toJSON: () => ({}),
    });
    const panelSurface = container.querySelector<HTMLElement>(
      'section[aria-label="Live requests"]',
    );

    expect(screen.queryByRole('button', { name: 'Drag Live requests' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Resize Live requests' })).toBeNull();
    expect(container.querySelector('[data-dashboard-resize-handle]')).toBeTruthy();

    fireEvent.doubleClick(panelSurface!);
    expect(onOpenPanel).not.toHaveBeenCalled();

    firePointerEvent(panelSurface!, 'pointerdown', {
      pointerId: 21,
      clientX: 100,
      clientY: 50,
    });
    firePointerEvent(window, 'pointerup', {
      pointerId: 21,
      clientX: 100,
      clientY: 50,
    });
    expect(onSelect).toHaveBeenCalledWith(new Set(['panel-live']));
    expect(onCommitElements).not.toHaveBeenCalled();

    firePointerEvent(panelSurface!, 'pointerdown', {
      pointerId: 22,
      clientX: 100,
      clientY: 50,
    });
    firePointerEvent(window, 'pointermove', {
      pointerId: 22,
      clientX: 220,
      clientY: 50,
    });
    firePointerEvent(window, 'pointerup', {
      pointerId: 22,
      clientX: 220,
      clientY: 50,
    });
    const [nextElements] = onCommitElements.mock.calls.at(-1) ?? [];
    expect(nextElements?.[0]?.gridPos.x).toBeGreaterThan(0);

    await user.click(
      screen.getByRole('button', { name: 'Open panel menu: Live requests' }),
    );
    await user.click(screen.getByRole('menuitem', { name: 'Edit' }));
    expect(onOpenPanel).toHaveBeenCalledWith('panel-live');
  });
});

function firePointerEvent(
  target: Window | Document | Node,
  type: 'pointerdown' | 'pointermove' | 'pointerup',
  options: { pointerId: number; clientX: number; clientY: number },
): void {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    button: 0,
    clientX: options.clientX,
    clientY: options.clientY,
  });
  Object.defineProperty(event, 'pointerId', {
    configurable: true,
    value: options.pointerId,
  });
  fireEvent(target, event);
}
