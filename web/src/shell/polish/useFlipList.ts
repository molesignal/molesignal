import { type RefObject, useCallback, useLayoutEffect, useRef } from 'react';

/** A reordered item slides to its place this fast and stops there: no overshoot, no bounce. */
const FLIP_MS = 140;
const FLIP_EASING = 'cubic-bezier(0.2, 0.8, 0.2, 1)';

function measure(list: HTMLElement): Map<string, number> {
  const origin = list.getBoundingClientRect().top;
  const tops = new Map<string, number>();
  for (const item of list.querySelectorAll<HTMLElement>('[data-flip-key]')) {
    const key = item.dataset.flipKey;
    if (key) tops.set(key, item.getBoundingClientRect().top - origin);
  }
  return tops;
}

/**
 * Lets the items of a list travel to where a reorder put them instead of jumping there. Items
 * are matched across renders by `data-flip-key`, and a change of `order` plays the slide from
 * where they stood when the list last laid out, or when `capture` (returned) was last called:
 * call it just before a reorder in a list whose owner does not re-render while it is open (a
 * menu). Positions are taken against the list itself, so a scroll does not read as a move.
 */
export function useFlipList(ref: RefObject<HTMLElement | null>, order: string): () => void {
  const tops = useRef(new Map<string, number>());

  const capture = useCallback(() => {
    if (ref.current) tops.current = measure(ref.current);
  }, [ref]);

  useLayoutEffect(() => {
    const list = ref.current;
    if (!list) return;
    const now = measure(list);
    const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
    for (const item of list.querySelectorAll<HTMLElement>('[data-flip-key]')) {
      const key = item.dataset.flipKey ?? '';
      const before = tops.current.get(key);
      const top = now.get(key);
      if (before === undefined || top === undefined || reduce || Math.abs(before - top) < 0.5) continue;
      if (typeof item.animate !== 'function') continue;
      item.animate(
        [{ transform: `translateY(${before - top}px)` }, { transform: 'translateY(0)' }],
        { duration: FLIP_MS, easing: FLIP_EASING },
      );
    }
    tops.current = now;
  }, [ref, order]);

  return capture;
}
