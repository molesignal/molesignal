import * as React from 'react';

import { cn } from '@/shell/lib/cn';

/** How far an arrow key moves the edge; twice that with Shift. */
const KEY_STEP = 24;

interface ResizeHandleProps {
  /** What the edge resizes, for assistive technology and the tip. */
  label: string;
  width: number;
  min: number;
  max: number;
  /**
   * Which way a drag to the right moves the width: an edge on the right of
   * what it sizes grows it, one on its left shrinks it.
   */
  grows: 'right' | 'left';
  /** The width while it is dragged, and after each key. */
  onWidthChange: (width: number) => void;
  /** The width once a drag is let go: the moment to keep it. */
  onWidthCommit?: ((width: number) => void) | undefined;
  /** What a double click goes back to. */
  defaultWidth?: number | undefined;
  /** No line at rest: the panes' own edge shows where it is. */
  quiet?: boolean | undefined;
  className?: string | undefined;
}

/**
 * The edge between two panes, dragged or moved with the arrow keys to share
 * the room between them. A line shows under the pointer, under the keyboard
 * and while it is dragged: no frame, no ring.
 */
export function ResizeHandle({
  label,
  width,
  min,
  max,
  grows,
  onWidthChange,
  onWidthCommit,
  defaultWidth,
  quiet = false,
  className,
}: ResizeHandleProps) {
  const [dragging, setDragging] = React.useState(false);
  const widthRef = React.useRef(width);
  widthRef.current = width;
  const cleanupRef = React.useRef<(() => void) | null>(null);

  React.useEffect(() => () => cleanupRef.current?.(), []);

  const clamp = React.useCallback((next: number) => Math.round(Math.min(max, Math.max(min, next))), [max, min]);

  const begin = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    cleanupRef.current?.();

    const pointerId = event.pointerId;
    const startX = event.clientX;
    const startWidth = widthRef.current;
    const direction = grows === 'right' ? 1 : -1;
    const previousCursor = document.body.style.cursor;
    const previousSelect = document.body.style.userSelect;
    let latest = startWidth;

    const move = (pointer: PointerEvent) => {
      if (pointer.pointerId !== pointerId) return;
      pointer.preventDefault();
      latest = clamp(startWidth + (pointer.clientX - startX) * direction);
      onWidthChange(latest);
    };

    const finish = (pointer: PointerEvent) => {
      if (pointer.pointerId !== pointerId) return;
      cleanup();
      setDragging(false);
      onWidthCommit?.(latest);
    };

    function cleanup() {
      globalThis.removeEventListener('pointermove', move);
      globalThis.removeEventListener('pointerup', finish);
      globalThis.removeEventListener('pointercancel', finish);
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousSelect;
      cleanupRef.current = null;
    }

    cleanupRef.current = cleanup;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    setDragging(true);
    globalThis.addEventListener('pointermove', move, { passive: false });
    globalThis.addEventListener('pointerup', finish);
    globalThis.addEventListener('pointercancel', finish);
  };

  const fromKeyboard = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const step = KEY_STEP * (event.shiftKey ? 2 : 1) * (grows === 'right' ? 1 : -1);
    let next: number | null = null;

    if (event.key === 'ArrowLeft') next = widthRef.current - step;
    if (event.key === 'ArrowRight') next = widthRef.current + step;
    if (event.key === 'Home') next = grows === 'right' ? min : max;
    if (event.key === 'End') next = grows === 'right' ? max : min;
    if (next === null) return;

    event.preventDefault();
    const width = clamp(next);
    onWidthChange(width);
    onWidthCommit?.(width);
  };

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={Math.round(width)}
      aria-valuetext={`${Math.round(width)}px`}
      tabIndex={0}
      title={label}
      data-resizing={dragging || undefined}
      onPointerDown={begin}
      onKeyDown={fromKeyboard}
      onDoubleClick={() => {
        if (defaultWidth === undefined) return;
        const width = clamp(defaultWidth);
        onWidthChange(width);
        onWidthCommit?.(width);
      }}
      className={cn(
        'group relative z-20 w-[9px] shrink-0 cursor-col-resize touch-none select-none focus-visible:outline-none',
        className,
      )}
    >
      <span
        aria-hidden="true"
        className={cn(
          'absolute inset-y-0 left-1/2 w-px -translate-x-1/2 transition-colors duration-fast',
          quiet ? 'bg-transparent' : 'bg-bd-0',
          'group-hover:bg-indigo group-focus-visible:bg-indigo',
          dragging && 'bg-indigo',
        )}
      />
    </div>
  );
}
