import * as React from 'react';

/** How long the pointer rests on the button before the menu opens: a pass across it does not. */
export const HOVER_OPEN_DELAY_MS = 120;
/** How long the menu waits once the pointer has left it and its button, so a path between the two keeps it. */
export const HOVER_CLOSE_DELAY_MS = 250;

export interface HoverOpen {
  /** For the button and the panel: a mouse coming in opens the menu, or keeps it. */
  onPointerEnter: (event: React.PointerEvent) => void;
  /** For the button and the panel: a mouse going out closes a menu it opened. */
  onPointerLeave: (event: React.PointerEvent) => void;
  /** The button's click: it pins a menu the mouse opened, and otherwise opens or closes it. */
  toggle: () => void;
}

/**
 * Opens the main menu when a mouse rests on its button, and closes it when the mouse has left the
 * button and the menu both. A menu opened by a click, a touch or the keyboard stays until it is
 * closed the usual ways; a click on the button while the mouse holds it open pins it.
 */
export function useHoverOpen(open: boolean, setOpen: React.Dispatch<React.SetStateAction<boolean>>): HoverOpen {
  const openTimer = React.useRef<ReturnType<typeof setTimeout>>();
  const closeTimer = React.useRef<ReturnType<typeof setTimeout>>();
  const byHover = React.useRef(false);
  const openRef = React.useRef(open);
  openRef.current = open;

  React.useEffect(() => {
    if (!open) byHover.current = false;
  }, [open]);

  React.useEffect(
    () => () => {
      clearTimeout(openTimer.current);
      clearTimeout(closeTimer.current);
    },
    [],
  );

  const onPointerEnter = React.useCallback(
    (event: React.PointerEvent) => {
      if (event.pointerType !== 'mouse') return;
      clearTimeout(closeTimer.current);
      if (openRef.current) return;
      clearTimeout(openTimer.current);
      openTimer.current = setTimeout(() => {
        byHover.current = true;
        setOpen(true);
      }, HOVER_OPEN_DELAY_MS);
    },
    [setOpen],
  );

  const onPointerLeave = React.useCallback(
    (event: React.PointerEvent) => {
      if (event.pointerType !== 'mouse') return;
      clearTimeout(openTimer.current);
      if (!byHover.current) return;
      clearTimeout(closeTimer.current);
      closeTimer.current = setTimeout(() => setOpen(false), HOVER_CLOSE_DELAY_MS);
    },
    [setOpen],
  );

  const toggle = React.useCallback(() => {
    clearTimeout(openTimer.current);
    clearTimeout(closeTimer.current);
    if (openRef.current && byHover.current) {
      byHover.current = false;
      return;
    }
    setOpen((current) => !current);
  }, [setOpen]);

  return { onPointerEnter, onPointerLeave, toggle };
}
