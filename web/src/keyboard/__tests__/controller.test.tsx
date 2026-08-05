import { fireEvent, render } from '@testing-library/react';
import * as React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { KeyboardProvider, useBindings, useScope } from '@/keyboard/controller';
import { useKeyboardScope } from '@/stores/useKeyboardScope';

afterEach(() => useKeyboardScope.setState({ stack: ['global'] }));

function Wrap({ children }: { children: React.ReactNode }) {
  return <KeyboardProvider>{children}</KeyboardProvider>;
}

describe('keyboard controller', () => {
  it('scope stack pushes/pops via useScope', () => {
    function Child() {
      useScope('drawer', true);
      return <div />;
    }
    const { unmount } = render(<Child />, { wrapper: Wrap });
    expect(useKeyboardScope.getState().stack.includes('drawer')).toBe(true);
    unmount();
    expect(useKeyboardScope.getState().stack.includes('drawer')).toBe(false);
  });

  it('dispatches a binding when its modified combo is pressed inside the active scope', () => {
    const handler = vi.fn();
    function Child() {
      useBindings('global', [{ keys: 'mod+alt+x', handler, description: 'test' }]);
      return <div data-testid="root" tabIndex={0} />;
    }
    const { getByTestId } = render(<Child />, { wrapper: Wrap });
    getByTestId('root').focus();
    fireEvent.keyDown(window, { key: 'x', metaKey: true, altKey: true });
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it('does not dispatch unmodified keys even when registered', () => {
    const handler = vi.fn();
    function Child() {
      useBindings('global', [{ keys: 'x', handler, description: 'test' }]);
      return <div />;
    }
    render(<Child />, { wrapper: Wrap });
    fireEvent.keyDown(window, { key: 'x' });
    expect(handler).not.toHaveBeenCalled();
  });
});
