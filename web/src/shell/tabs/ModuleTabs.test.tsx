import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { Activity } from 'lucide-react';
import * as React from 'react';
import { MemoryRouter, Route, Routes, useNavigate } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ModuleTabs, type ModuleTabItem } from './ModuleTabs';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const ITEMS: ModuleTabItem[] = [
  { key: 'overview', label: 'Overview', to: '/m/overview', group: 'a', testId: 'overview' },
  { key: 'checks', label: 'Checks', to: '/m/checks', group: 'a', icon: Activity, testId: 'checks' },
  { key: 'results', label: 'Results', to: '/m/results', group: 'b', count: 12, testId: 'results' },
];

function renderAt(path: string, props: Partial<React.ComponentProps<typeof ModuleTabs>> = {}) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route
          path="*"
          element={<ModuleTabs label="Module views" items={ITEMS} {...props} />}
        />
      </Routes>
    </MemoryRouter>,
  );
}

describe('ModuleTabs as navigation', () => {
  it('is one navigation landmark of links, with the current page marked', () => {
    renderAt('/m/checks/abc');

    const nav = screen.getByRole('navigation', { name: 'Module views' });
    const links = within(nav).getAllByRole('link');
    expect(links).toHaveLength(3);
    expect(links.map((link) => link.getAttribute('data-testid'))).toEqual(['overview', 'checks', 'results']);
    expect(links.map((link) => link.getAttribute('aria-current'))).toEqual([null, 'page', null]);
  });

  it('names a tab by its label and count, not by the invisible bold copy of the label', () => {
    renderAt('/m/results');

    expect(screen.getByRole('link', { name: 'Results 12' })).toHaveAttribute('aria-current', 'page');
  });

  it('sets groups of tabs apart with a divider, and no other pair', () => {
    const { container } = renderAt('/m/overview');

    const children = Array.from(container.querySelectorAll('[data-tab], [data-tab-divider]'));
    expect(
      children.map((node) => (node.hasAttribute('data-tab-divider') ? '|' : node.getAttribute('data-testid'))),
    ).toEqual(['overview', 'checks', '|', 'results']);
  });

  it('puts trailing tabs at the far end', () => {
    const { container } = renderAt('/m/overview', {
      trailing: [{ key: 'settings', label: 'Settings', to: '/m/settings' }],
    });

    const links = container.querySelectorAll('a[data-tab]');
    const last = links[links.length - 1] as HTMLElement;
    expect(last).toHaveAccessibleName('Settings');
    expect(last.parentElement?.className).toContain('ml-auto');
  });

  it('lets a tab that owns several routes stay lit on all of them', () => {
    renderAt('/m/results', {
      items: [
        { key: 'a', label: 'A', to: '/m/a' },
        { key: 'b', label: 'B', to: '/m/b', active: true },
      ],
    });

    expect(screen.getByRole('link', { name: 'B' })).toHaveAttribute('aria-current', 'page');
    expect(screen.getByRole('link', { name: 'A' })).not.toHaveAttribute('aria-current');
  });

  it('moves focus between the tabs with the arrow keys, without following the links', () => {
    renderAt('/m/overview');
    const [first, second, third] = screen.getAllByRole('link');
    first?.focus();

    fireEvent.keyDown(first!, { key: 'ArrowRight' });
    expect(second).toHaveFocus();
    fireEvent.keyDown(second!, { key: 'End' });
    expect(third).toHaveFocus();
    fireEvent.keyDown(third!, { key: 'ArrowRight' });
    expect(first).toHaveFocus();
    fireEvent.keyDown(first!, { key: 'ArrowLeft' });
    expect(third).toHaveFocus();
    fireEvent.keyDown(third!, { key: 'Home' });
    expect(first).toHaveFocus();
    // Still on the first page: arrows only moved focus.
    expect(first).toHaveAttribute('aria-current', 'page');
  });

  it('can be a card of its own or the header row of an existing one', () => {
    const { container, rerender } = renderAt('/m/overview', { dataAttributes: { 'data-x': 'y' } });
    expect(container.firstElementChild?.className).toContain('rounded-md');
    expect(container.firstElementChild?.className).toContain('shadow-functional-surface');
    expect(container.firstElementChild).toHaveAttribute('data-x', 'y');

    rerender(
      <MemoryRouter initialEntries={['/m/overview']}>
        <ModuleTabs label="Module views" items={ITEMS} variant="inline" />
      </MemoryRouter>,
    );
    expect(container.firstElementChild?.className ?? '').not.toContain('shadow');
  });

  it('holds a second row of tabs in the same card', () => {
    renderAt('/m/overview', { children: <p>second row</p> });

    const card = screen.getByRole('navigation', { name: 'Module views' }).parentElement;
    expect(card).toContainElement(screen.getByText('second row'));
  });
});

describe('ModuleTabs that switch content in the page', () => {
  function Harness({ onChange }: { onChange: (key: string) => void }) {
    const [value, setValue] = React.useState('overview');
    return (
      <MemoryRouter>
        <ModuleTabs
          label="Module views"
          items={ITEMS.map(({ to: _to, ...item }) => item)}
          value={value}
          onValueChange={(key) => {
            setValue(key);
            onChange(key);
          }}
        />
      </MemoryRouter>
    );
  }

  it('is a tab list, with the selected tab marked and the only one in the tab order', () => {
    render(<Harness onChange={vi.fn()} />);

    const list = screen.getByRole('tablist', { name: 'Module views' });
    const tabs = within(list).getAllByRole('tab');
    expect(tabs.map((tab) => tab.getAttribute('aria-selected'))).toEqual(['true', 'false', 'false']);
    expect(tabs.map((tab) => tab.getAttribute('tabindex'))).toEqual(['0', '-1', '-1']);
  });

  it('selects a tab on click', () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);

    fireEvent.click(screen.getByRole('tab', { name: 'Checks' }));

    expect(onChange).toHaveBeenCalledWith('checks');
    expect(screen.getByRole('tab', { name: 'Checks' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByRole('tab', { name: 'Checks' })).toHaveAttribute('tabindex', '0');
  });

  it('selects the tab an arrow key lands on', () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    const overview = screen.getByRole('tab', { name: 'Overview' });
    overview.focus();

    fireEvent.keyDown(overview, { key: 'ArrowRight' });

    expect(screen.getByRole('tab', { name: 'Checks' })).toHaveFocus();
    expect(onChange).toHaveBeenLastCalledWith('checks');
  });
});

describe('ModuleTabs indicator', () => {
  // jsdom has no layout: every tab is 100px wide and they sit side by side.
  function layOut() {
    const indexOf = (element: HTMLElement) =>
      Array.from(element.parentElement?.querySelectorAll('[data-tab]') ?? []).indexOf(element);
    vi.spyOn(HTMLElement.prototype, 'offsetLeft', 'get').mockImplementation(function (this: HTMLElement) {
      return this.hasAttribute('data-tab') ? indexOf(this) * 100 : 0;
    });
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) {
      return this.hasAttribute('data-tab') ? 100 : 0;
    });
  }

  const indicator = (container: HTMLElement) =>
    container.querySelector<HTMLElement>('[data-tab-indicator]')!;

  it('sits under the active tab, inset from its edges', () => {
    layOut();
    const { container } = renderAt('/m/checks');

    expect(indicator(container).style.transform).toBe('translateX(106px)');
    expect(indicator(container).style.width).toBe('88px');
  });

  it('glides to the next tab when the route changes', () => {
    layOut();
    function Jump() {
      const navigate = useNavigate();
      return <button onClick={() => navigate('/m/results')}>go</button>;
    }
    const { container } = render(
      <MemoryRouter initialEntries={['/m/overview']}>
        <Jump />
        <ModuleTabs label="Module views" items={ITEMS} />
      </MemoryRouter>,
    );
    expect(indicator(container).style.transform).toBe('translateX(6px)');

    fireEvent.click(screen.getByText('go'));

    expect(indicator(container).style.transform).toBe('translateX(206px)');
  });

  it('is hidden when no tab is active', () => {
    layOut();
    const { container } = renderAt('/elsewhere');

    expect(indicator(container).className).toContain('opacity-0');
    expect(indicator(container).getAttribute('style')).toBeNull();
  });

  it('does not slide in from the edge on first paint', async () => {
    layOut();
    const { container } = renderAt('/m/results');

    expect(indicator(container).className).not.toContain('transition');
    await act(async () => {
      await new Promise((resolve) => window.requestAnimationFrame(() => resolve(null)));
    });
    expect(indicator(container).className).toContain('transition');
  });
});
