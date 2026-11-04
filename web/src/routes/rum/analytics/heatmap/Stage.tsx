import { Replayer } from '@rrweb/replay';
import '@rrweb/replay/dist/style.css';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ClickMap, HeatmapSnapshot, ScrollMap } from '@/api/rum';

import { drawClicks, drawScroll } from './draw';
import { type Box, depthReached, type MapKind, spotsOf } from './model';

/** The share of the views the lines of a scroll map are drawn at. */
const LINES = [0.75, 0.5, 0.25] as const;

/** How often a page that is still laying itself out is measured again. */
const SETTLE_MS = [0, 200, 800];

interface Frame {
  width: number;
  height: number;
  scale: number;
}

/** Finds an element in the recorded page. A selector that does not parse finds nothing. */
function locator(doc: Document | null): (element: string) => Box | null {
  return (element) => {
    if (!doc) return null;

    try {
      const found = doc.querySelector(element);

      if (!found) return null;

      const box = found.getBoundingClientRect();
      const left = box.left + (doc.documentElement?.scrollLeft ?? 0);
      const top = box.top + (doc.documentElement?.scrollTop ?? 0);

      return box.width > 0 || box.height > 0 ? { left, top, width: box.width, height: box.height } : null;
    } catch {
      return null;
    }
  };
}

/**
 * The page as it was recorded, with a map over it. The recording is held at the moment the page had
 * loaded, with the whole length of the page laid out, and shrunk to the width there is: nothing in it can
 * be clicked. What is masked in a replay is masked here too.
 */
export function HeatmapStage({
  snapshot,
  kind,
  clicks,
  scroll,
  onLocated,
}: {
  snapshot: HeatmapSnapshot;
  kind: MapKind;
  clicks: ClickMap | undefined;
  scroll: ScrollMap | undefined;
  /** The elements of the map the recording does not have, once it is known. */
  onLocated: (off: ReadonlySet<string>) => void;
}) {
  const { t } = useTranslation('rum');
  const stageRef = React.useRef<HTMLDivElement>(null);
  const rootRef = React.useRef<HTMLDivElement>(null);
  const canvasRef = React.useRef<HTMLCanvasElement>(null);
  const playerRef = React.useRef<Replayer | undefined>(undefined);
  const [frame, setFrame] = React.useState<Frame | null>(null);
  const [failure, setFailure] = React.useState<string>();

  React.useEffect(() => {
    const root = rootRef.current;
    const stage = stageRef.current;

    if (!root || !stage) return;

    root.replaceChildren();
    setFrame(null);
    setFailure(undefined);

    const timers: number[] = [];
    let player: Replayer | undefined;

    try {
      player = new Replayer(snapshot.events as unknown as ConstructorParameters<typeof Replayer>[0], {
        root,
        skipInactive: false,
        showWarning: false,
        showDebug: false,
        mouseTail: false,
        triggerFocus: false,
        UNSAFE_replayCanvas: false,
      });

      player.disableInteract();
      playerRef.current = player;

      const shown = player;
      const origin = shown.getMetaData().startTime;

      shown.pause(Math.max(0, snapshot.atTime / 1_000 - origin));

      const measure = () => {
        const doc = shown.iframe.contentDocument;
        const width = Number(shown.iframe.width) || (snapshot.width ?? 0);
        const page = doc?.documentElement?.scrollHeight ?? 0;
        const height = Math.max(Number(shown.iframe.height) || 0, page);

        if (!width || !height) return;

        // The whole length of the page, so that nothing is left to scroll to inside it.
        shown.iframe.style.height = `${height}px`;
        shown.iframe.setAttribute('height', String(height));

        const scale = Math.min(1, stage.clientWidth / width);

        shown.wrapper.style.transformOrigin = 'top left';
        shown.wrapper.style.transform = `scale(${scale})`;
        root.style.width = `${width * scale}px`;
        root.style.height = `${height * scale}px`;

        setFrame({ width, height, scale });
      };

      for (const delay of SETTLE_MS) timers.push(window.setTimeout(measure, delay));
    } catch (cause) {
      setFailure(cause instanceof Error ? cause.message : String(cause));
    }

    return () => {
      timers.forEach((timer) => window.clearTimeout(timer));
      player?.destroy();
      playerRef.current = undefined;
      root.replaceChildren();
    };
  }, [snapshot]);

  React.useEffect(() => {
    const canvas = canvasRef.current;

    if (!canvas || !frame) return;

    canvas.width = Math.round(frame.width * frame.scale);
    canvas.height = Math.round(frame.height * frame.scale);

    if (kind === 'scroll') {
      drawScroll(canvas, scroll?.bands ?? [], frame.scale);
      onLocated(new Set());

      return;
    }

    const { spots, off } = spotsOf(clicks?.elements ?? [], locator(playerRef.current?.iframe.contentDocument ?? null));

    drawClicks(canvas, spots, frame.scale);
    onLocated(off);
    canvas.dataset.heatSpots = String(spots.length);
  }, [clicks, frame, kind, onLocated, scroll]);

  if (failure) {
    return (
      <div data-rum-heatmap-failed="" className="grid min-h-60 place-items-center p-6 text-center font-sans text-xs text-tx-3">
        {t('analytics.heatmap.stage_failed', { reason: failure })}
      </div>
    );
  }

  return (
    <div ref={stageRef} data-rum-heatmap-stage={kind} className="relative max-h-[calc(100vh-260px)] min-h-60 overflow-auto bg-bg-3 [&_.replayer-mouse]:hidden">
      <div className="relative">
        <div ref={rootRef} aria-hidden className="pointer-events-none relative" />
        <canvas ref={canvasRef} data-rum-heatmap-canvas="" aria-hidden className="pointer-events-none absolute left-0 top-0" />
        {kind === 'scroll' && frame && (
          <div aria-hidden className="pointer-events-none absolute inset-0">
            {LINES.map((share) => {
              const depth = depthReached(scroll?.bands ?? [], share);

              return depth === null ? null : (
                <div key={share} data-rum-scroll-line={share} className="absolute inset-x-0 border-t border-dashed border-tx-0/80" style={{ top: depth * frame.scale }}>
                  <span className="absolute right-2 top-1 rounded-sm bg-bg-1 px-1.5 py-0.5 font-sans text-xs text-tx-0">
                    {t('analytics.heatmap.scroll_line', { share: Math.round(share * 100) })}
                  </span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
