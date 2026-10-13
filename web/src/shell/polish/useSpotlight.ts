import * as React from 'react';

/**
 * One pointer listener for the whole shell: while the pointer moves over an
 * element marked `data-spotlight`, its --mx / --my follow the cursor so the
 * card's CSS can draw a light under it (see surfaces.css). Coarse pointers have
 * no hover, so nothing is attached for them.
 */
export function useSpotlight(): void {
  React.useEffect(() => {
    if (typeof window.matchMedia !== 'function') return;
    if (!window.matchMedia('(hover: hover)').matches) return;

    let frame = 0;
    const onMove = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      const card = target.closest<HTMLElement>('[data-spotlight]');
      if (!card) return;
      const { clientX, clientY } = event;
      // One write per frame, however fast the pointer moves.
      window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(() => {
        const box = card.getBoundingClientRect();
        card.style.setProperty('--mx', `${clientX - box.left}px`);
        card.style.setProperty('--my', `${clientY - box.top}px`);
      });
    };

    document.addEventListener('pointermove', onMove, { passive: true });
    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener('pointermove', onMove);
    };
  }, []);
}
