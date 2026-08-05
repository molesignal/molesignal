import * as React from 'react';
import { createPortal } from 'react-dom';

import { type Frame, useInvestigationStack } from '@/stores/useInvestigationStack';

import { DrawerFrame } from './DrawerFrame';

const STAGGER_MS = 50;
const ANIM_MS = 200;

interface ExitingEntry {
  frame: Frame;
  /** Index in the original stack at the time of removal. */
  index: number;
  /** Total stack size at the time of removal. */
  total: number;
  /** Timestamp (perf.now()) when this exit animation should start. */
  startAt: number;
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
 * 50ms-staggered slide-out cascade, we mirror the previous stack in a ref
 * and, on any removal, push the gone frames onto an `exiting` list with
 * staggered `startAt` timestamps. Each exiting drawer renders with the
 * `animate-slide-out-right` keyframe; after `STAGGER_MS * i + ANIM_MS` it
 * is dropped from `exiting`.
 */
export function StackPortal() {
  const frames = useInvestigationStack((s) => s.frames);
  const prevRef = React.useRef<Frame[]>([]);
  const [exiting, setExiting] = React.useState<ExitingEntry[]>([]);

  React.useEffect(() => {
    const prev = prevRef.current;
    const prevTotal = prev.length;
    const nowIds = new Set(frames.map((f) => f.id));
    const gone = prev.filter((f) => !nowIds.has(f.id));
    if (gone.length > 0) {
      const t0 = performance.now();
      const fresh = gone.map<ExitingEntry>((frame, i) => {
        // Higher original index slides out first (top drawer leaves before
        // the layers beneath it).
        const originalIdx = prev.findIndex((p) => p.id === frame.id);
        return { frame, index: originalIdx, total: prevTotal, startAt: t0 + i * STAGGER_MS };
      });
      setExiting((e) => [...e, ...fresh]);
      // Schedule per-entry removal after stagger + animation.
      fresh.forEach((entry, i) => {
        const totalDelay = i * STAGGER_MS + ANIM_MS;
        window.setTimeout(() => {
          setExiting((cur) => cur.filter((x) => x.frame.id !== entry.frame.id));
        }, totalDelay);
      });
    }
    prevRef.current = frames;
  }, [frames]);

  if (frames.length === 0 && exiting.length === 0) return null;

  return createPortal(
    <>
      {/* Dim only when there's at least one live drawer. */}
      {frames.length > 0 && <div className="fixed inset-0 z-40 bg-overlay-soft" aria-hidden />}
      {frames.map((frame, i) => (
        <DrawerFrame
          key={frame.id}
          frame={frame}
          index={i}
          total={frames.length}
          isTop={i === frames.length - 1}
        />
      ))}
      {exiting.map((e) => (
        <ExitingDrawer key={e.frame.id} entry={e} />
      ))}
    </>,
    document.body,
  );
}

/**
 * Wraps a soon-to-unmount DrawerFrame with the slide-out keyframe + a
 * stagger delay computed from the exit entry's `startAt`. After
 * `ANIM_MS` it is removed from the parent's `exiting` state and unmounts.
 */
function ExitingDrawer({ entry }: { entry: ExitingEntry }) {
  const delay = Math.max(0, entry.startAt - performance.now());
  return (
    <div
      style={{ animationDelay: `${delay}ms` }}
      className="animate-slide-out-right pointer-events-none"
    >
      <DrawerFrame frame={entry.frame} index={entry.index} total={entry.total} isTop={false} />
    </div>
  );
}
