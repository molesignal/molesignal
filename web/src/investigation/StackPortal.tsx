import * as React from 'react';
import { createPortal } from 'react-dom';

import { type Frame, useInvestigationStack } from '@/stores/useInvestigationStack';

import { DrawerFrame } from './DrawerFrame';

const STAGGER_MS = 50;
const ANIM_MS = 200;

/** The drawer of a frame that left the stack, for as long as it slides out. */
interface LeavingDrawer {
  frame: Frame;
  /** Index in the stack the frame left. */
  index: number;
  /** Size of the stack the frame left. */
  total: number;
  /** How long the drawer waits before it starts to move. */
  delayMs: number;
  /** Timestamp (perf.now()) at which the slide-out is over and the drawer unmounts. */
  goneAt: number;
}

/**
 * Renders the investigation stack as a fixed series of right-aligned drawers.
 * The first frame (`frames[0]`) is the bottom of the stack; the last frame is
 * the top and gets focus. Lower frames peek 32px from the right so the user
 * can click them to cascade-pop.
 *
 * Mounted via React Portal directly on document.body so the drawers float
 * over any route content without inheriting layout context.
 *
 * Cascade-pop animation
 * ---------------------
 * The store's `popTo` removes frames synchronously. To get the spec'd
 * 50ms-staggered slide-out cascade, the drawers of the frames that left stay
 * in the rendered list as `leaving`, the top one moving first, and slide out
 * to the right (`DrawerFrame`); after `delayMs + ANIM_MS` an entry is dropped
 * and its drawer unmounts.
 *
 * A leaving drawer is the drawer the frame already had: same list, same key.
 * Mounting a second one for the animation would run the frame's content, and
 * its queries, once more on the way out.
 */
export function StackPortal() {
  const frames = useInvestigationStack((s) => s.frames);
  const leaving = useLeavingDrawers(frames);

  if (frames.length === 0 && leaving.length === 0) return null;

  return createPortal(
    <>
      {/* Dim only when there's at least one live drawer. */}
      {frames.length > 0 && <div className="fixed inset-0 z-40 bg-overlay-soft" aria-hidden />}
      {[
        ...frames.map((frame, i) => (
          <DrawerFrame
            key={frame.id}
            frame={frame}
            index={i}
            total={frames.length}
            isTop={i === frames.length - 1}
          />
        )),
        ...leaving.map((entry) => (
          <DrawerFrame
            key={entry.frame.id}
            frame={entry.frame}
            index={entry.index}
            total={entry.total}
            isTop={false}
            leaveDelayMs={entry.delayMs}
          />
        )),
      ]}
    </>,
    document.body,
  );
}

/** The drawers that are still sliding out after their frames left `frames`. */
function useLeavingDrawers(frames: Frame[]): LeavingDrawer[] {
  const [seen, setSeen] = React.useState(frames);
  const [leaving, setLeaving] = React.useState<LeavingDrawer[]>([]);

  // Worked out while rendering, not in an effect after it: the drawer of a
  // frame that has just left has to be in this very render. One render without
  // it and React unmounts the drawer, and there is nothing left to slide out.
  if (seen !== frames) {
    setSeen(frames);
    setLeaving(afterStackChange(leaving, seen, frames));
  }

  React.useEffect(() => {
    const timers = leaving.map((entry) =>
      window.setTimeout(
        () => setLeaving((entries) => entries.filter((other) => other !== entry)),
        entry.goneAt - performance.now(),
      ),
    );
    return () => timers.forEach((timer) => window.clearTimeout(timer));
  }, [leaving]);

  return leaving;
}

function afterStackChange(leaving: LeavingDrawer[], before: Frame[], now: Frame[]): LeavingDrawer[] {
  const open = new Set(now.map((frame) => frame.id));
  // A frame that is back before its drawer was gone is simply open again.
  const stillLeaving = leaving.filter((entry) => !open.has(entry.frame.id));
  const gone = before.filter((frame) => !open.has(frame.id));
  if (gone.length === 0 && stillLeaving.length === leaving.length) return leaving;

  const t0 = performance.now();
  return [
    ...stillLeaving,
    ...gone.map((frame, i) => {
      // The top drawer leaves first, the layers beneath it follow.
      const delayMs = (gone.length - 1 - i) * STAGGER_MS;
      return {
        frame,
        index: before.indexOf(frame),
        total: before.length,
        delayMs,
        goneAt: t0 + delayMs + ANIM_MS,
      };
    }),
  ];
}
