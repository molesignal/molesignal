import * as React from 'react';

/**
 * True when motion should be skipped. Browsers that cannot answer the media
 * query (and jsdom) are treated as "no motion" so figures render their final
 * value immediately.
 */
function motionIsReduced(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return true;
  }
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * Eases a displayed figure toward `value`. The first render counts up from
 * zero; later changes tween from whatever is currently on screen.
 */
export function useEasedNumber(value: number, durationMs = 700): number {
  const [display, setDisplay] = React.useState(() =>
    motionIsReduced() || !Number.isFinite(value) ? value : 0,
  );
  const shownRef = React.useRef(display);

  React.useEffect(() => {
    if (!Number.isFinite(value) || motionIsReduced()) {
      shownRef.current = value;
      setDisplay(value);
      return;
    }
    const from = shownRef.current;
    if (from === value) return;

    let frame = 0;
    const start = performance.now();
    const tick = (now: number) => {
      const progress = Math.min(1, (now - start) / durationMs);
      const eased = 1 - (1 - progress) ** 3;
      const next = progress === 1 ? value : from + (value - from) * eased;
      shownRef.current = next;
      setDisplay(next);
      if (progress < 1) frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(frame);
  }, [value, durationMs]);

  return display;
}

export function AnimatedNumber({
  value,
  decimals = 0,
  className,
}: {
  value: number;
  decimals?: number;
  className?: string | undefined;
}) {
  const eased = useEasedNumber(value);
  return <span className={className}>{eased.toFixed(decimals)}</span>;
}
