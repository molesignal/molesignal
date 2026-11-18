import * as React from 'react';

/** The size of a button that names none, as the place it stands in asks for. */
const ButtonSizeContext = React.createContext<'default' | 'sm'>('default');

/**
 * Buttons that name no size of their own take `size`: a title row's actions are the small ones,
 * as tall as the controls in the cards under it, so they do not outweigh the title beside them.
 * `Button` and `ChromeButton` both read it.
 */
export function ButtonSize({ size, children }: { size: 'default' | 'sm'; children: React.ReactNode }) {
  return <ButtonSizeContext.Provider value={size}>{children}</ButtonSizeContext.Provider>;
}

/** The size the place a button stands in asks for, when the button names none. */
export function usePlacedButtonSize(): 'default' | 'sm' {
  return React.useContext(ButtonSizeContext);
}
