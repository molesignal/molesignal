import { Replayer } from '@rrweb/replay';
import '@rrweb/replay/dist/style.css';
import { AlertTriangle, LoaderCircle, Pause, Play, RotateCcw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ReplayEvent } from '@/api/rum';
import { ChromeButton, Pill } from '@/shell/chrome';

import { formatDurationNs } from '../_helpers';
import type { ReplayController } from './controller';
import type { PlayerEvent } from './events';

interface RrwebEvent extends Record<string, unknown> {
  type: number;
  timestamp: number;
  data: unknown;
}

/** A moment of the session worth a tick on the ruler: an error, a sign of frustration, a new page. */
export interface ReplayMark {
  /** In milliseconds since the epoch. */
  at: number;
  kind: 'error' | 'frustration' | 'view' | 'comment' | 'chapter';
}

export function ReplayPlayer({
  replayEvents,
  timelineEvents,
  durationNs,
  loading = false,
  loadError,
  controller,
  marks,
  startAt,
}: {
  replayEvents: ReplayEvent[];
  timelineEvents: PlayerEvent[];
  /** How long the session lasted, as the server derived it. */
  durationNs?: number | undefined;
  loading?: boolean;
  loadError?: unknown;
  /** What lets the rest of the page read where the replay is and move it. */
  controller?: ReplayController | undefined;
  /** The ticks on the ruler. */
  marks?: readonly ReplayMark[] | undefined;
  /** Where the replay opens, held there: a moment in milliseconds since the epoch. It opens at its beginning without one. */
  startAt?: number | undefined;
}) {
  const rrwebEvents = React.useMemo(() => validRrwebEvents(replayEvents), [replayEvents]);
  if (loading) return <ReplayRequestState loading />;
  if (loadError) return <ReplayRequestState error={loadError} />;
  const available = rrwebEvents.some((event) => event.type === 2);
  if (!available) {
    return <TimelineFallback events={timelineEvents} durationNs={durationNs} />;
  }
  return <RrwebReplay events={rrwebEvents} controller={controller} marks={marks} startAt={startAt} />;
}

function RrwebReplay({
  events,
  controller,
  marks,
  startAt,
}: {
  events: RrwebEvent[];
  controller: ReplayController | undefined;
  marks: readonly ReplayMark[] | undefined;
  startAt: number | undefined;
}) {
  const { t } = useTranslation('rum');
  const stageRef = React.useRef<HTMLDivElement>(null);
  const rootRef = React.useRef<HTMLDivElement>(null);
  const playerRef = React.useRef<Replayer | undefined>(undefined);
  const [duration, setDuration] = React.useState(0);
  const [position, setPosition] = React.useState(0);
  const [playing, setPlaying] = React.useState(false);
  const [speed, setSpeed] = React.useState(1);
  const [error, setError] = React.useState<string>();
  const [ready, setReady] = React.useState(false);
  // The recording is in order of time; its first event is where its clock starts.
  const origin = events[0]?.timestamp ?? 0;
  const openedAt = React.useRef<number | undefined>(undefined);

  React.useEffect(() => {
    const root = rootRef.current;
    const stage = stageRef.current;
    if (!root || !stage) return;
    root.replaceChildren();
    setError(undefined);
    setPlaying(false);
    setPosition(0);
    setReady(false);
    openedAt.current = undefined;

    try {
      const player = new Replayer(
        events as unknown as ConstructorParameters<typeof Replayer>[0],
        {
          root,
          // Preserve the recorded spacing between events; speed is only
          // changed when the viewer explicitly selects it.
          skipInactive: false,
          showWarning: false,
          showDebug: false,
          mouseTail: true,
          triggerFocus: false,
          UNSAFE_replayCanvas: false,
        },
      );
      player.disableInteract();
      playerRef.current = player;
      const metadata = player.getMetaData();
      setDuration(metadata.totalTime);
      setReady(true);

      const resize = () => {
        const meta = viewport(events);
        if (!meta) return;
        const scale = Math.min(
          stage.clientWidth / meta.width,
          stage.clientHeight / meta.height,
          1,
        );
        root.style.width = `${Math.max(1, meta.width * scale)}px`;
        root.style.height = `${Math.max(1, meta.height * scale)}px`;
        player.wrapper.style.transformOrigin = 'top left';
        player.wrapper.style.transform = `scale(${scale})`;
      };
      resize();
      const observer =
        typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(resize);
      observer?.observe(stage);
      player.on('finish', () => {
        setPlaying(false);
        setPosition(metadata.totalTime);
      });

      return () => {
        observer?.disconnect();
        player.destroy();
        playerRef.current = undefined;
        setReady(false);
        root.replaceChildren();
      };
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, [events]);

  React.useEffect(() => {
    if (!playing) return;
    const timer = window.setInterval(() => {
      const player = playerRef.current;
      if (player) setPosition(Math.min(duration, player.getCurrentTime()));
    }, 100);
    return () => window.clearInterval(timer);
  }, [duration, playing]);

  const seek = React.useCallback((next: number) => {
    const player = playerRef.current;
    if (!player) return;
    player.pause(next);
    setPosition(next);
    setPlaying(false);
  }, []);

  // The rest of the page reads where the replay is, and moves it.
  React.useEffect(() => {
    controller?.publish({ origin, duration, position, playing });
  }, [controller, origin, duration, position, playing]);

  React.useEffect(() => {
    if (!controller || !ready) return undefined;
    controller.attach(seek);

    return () => controller.attach(null);
  }, [controller, ready, seek]);

  // It opens where the address says, once, and waits there.
  React.useEffect(() => {
    if (!ready || startAt === undefined || openedAt.current === startAt) return;
    openedAt.current = startAt;
    seek(Math.min(duration, Math.max(0, startAt - origin)));
  }, [duration, origin, ready, seek, startAt]);

  const toggle = React.useCallback(() => {
    const player = playerRef.current;
    if (!player) return;
    if (playing) {
      player.pause();
      setPosition(player.getCurrentTime());
      setPlaying(false);
    } else {
      const start = position >= duration - 50 ? 0 : position;
      player.play(start);
      setPosition(start);
      setPlaying(true);
    }
  }, [duration, playing, position]);

  React.useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (
        (event.code !== 'Space' && event.key !== ' ') ||
        event.repeat ||
        event.defaultPrevented ||
        event.altKey ||
        event.ctrlKey ||
        event.metaKey ||
        event.shiftKey ||
        blocksReplaySpaceShortcut(event.target) ||
        !playerRef.current
      ) {
        return;
      }
      event.preventDefault();
      toggle();
    };
    window.addEventListener('keydown', handleKeyDown, true);
    return () => window.removeEventListener('keydown', handleKeyDown, true);
  }, [toggle]);

  const cycleSpeed = () => {
    const next = speed === 1 ? 2 : speed === 2 ? 4 : 1;
    playerRef.current?.setConfig({ speed: next });
    setSpeed(next);
  };

  const progress = duration > 0 ? Math.min(100, Math.max(0, (position / duration) * 100)) : 0;

  return (
    <div className="overflow-hidden rounded-lg border border-bd-1 bg-bg-1">
      <div className="flex min-h-11 flex-wrap items-center gap-3 border-b border-bd-0 px-4 py-2">
        <span className="text-sm font-display text-tx-0">
          {t('session_detail.replay_player')}
        </span>
        <Pill tone="green">{t('session_detail.replay_recorded')}</Pill>
        <span className="ml-auto text-xs text-tx-3">
          {events.length} {t('session_detail.events_unit')} · {formatReplayTime(duration)}
        </span>
      </div>

      {error ? (
        <div className="grid min-h-80 place-items-center p-8 text-center">
          <div>
            <AlertTriangle className="mx-auto h-6 w-6 text-red-soft" />
            <p className="mt-3 text-sm font-strong text-tx-0">
              {t('session_detail.replay_load_error')}
            </p>
            <p className="mt-1 max-w-lg break-words font-mono text-xs text-tx-3">{error}</p>
          </div>
        </div>
      ) : (
        <div
          ref={stageRef}
          className="grid h-[min(62vh,620px)] min-h-[360px] place-items-center overflow-hidden bg-bg-3"
        >
          <div ref={rootRef} className="relative" />
        </div>
      )}

      <div className="flex flex-wrap items-center gap-3 border-t border-bd-0 px-4 py-3">
        <ChromeButton
          size="sm"
          variant="ghost"
          onClick={() => seek(0)}
          aria-label={t('session_detail.restart')}
        >
          <RotateCcw className="h-4 w-4" />
        </ChromeButton>
        <button
          type="button"
          onClick={toggle}
          className="grid h-9 w-9 place-items-center rounded-full bg-indigo text-white hover:brightness-90 disabled:opacity-40"
          aria-label={playing ? t('session_detail.pause') : t('session_detail.play')}
          aria-keyshortcuts="Space"
          disabled={Boolean(error)}
        >
          {playing ? (
            <Pause className="h-4 w-4 fill-current" />
          ) : (
            <Play className="h-4 w-4 fill-current" />
          )}
        </button>
        <span className="min-w-24 shrink-0 whitespace-nowrap font-mono text-xs text-tx-3">
          {formatReplayTime(position)} / {formatReplayTime(duration)}
        </span>
        <div className="relative flex min-w-40 flex-1 items-center">
          <input
            type="range"
            min={0}
            max={Math.max(1, duration)}
            step={100}
            value={Math.min(position, Math.max(1, duration))}
            onChange={(event) => seek(Number(event.target.value))}
            className="h-[3px] w-full cursor-pointer appearance-none rounded-full bg-bd-1 disabled:cursor-not-allowed disabled:opacity-40 [&::-moz-range-progress]:h-[3px] [&::-moz-range-progress]:bg-transparent [&::-moz-range-thumb]:h-2.5 [&::-moz-range-thumb]:w-2.5 [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:border-0 [&::-moz-range-thumb]:bg-indigo [&::-moz-range-track]:h-[3px] [&::-moz-range-track]:bg-transparent [&::-webkit-slider-thumb]:h-2.5 [&::-webkit-slider-thumb]:w-2.5 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-indigo"
            style={{
              background: `linear-gradient(to right, var(--indigo) 0%, var(--indigo) ${progress}%, var(--bd-1) ${progress}%, var(--bd-1) 100%)`,
            }}
            aria-label={t('session_detail.seek')}
            disabled={Boolean(error)}
          />
          <Ticks marks={marks} origin={origin} duration={duration} />
        </div>
        <ChromeButton size="sm" variant="ghost" onClick={cycleSpeed}>
          {speed}×
        </ChromeButton>
      </div>
    </div>
  );
}

const TICK: Record<ReplayMark['kind'], string> = {
  error: 'h-[9px] bg-[var(--red)]',
  frustration: 'h-[9px] bg-[var(--orange)]',
  view: 'h-[13px] bg-[var(--tx-3)] opacity-60',
  comment: 'h-[9px] bg-[var(--blue)]',
  chapter: 'h-[13px] bg-[var(--indigo)] opacity-60',
};

/**
 * The moments worth finding on the ruler, as ticks across the track: errors, signs of frustration, the
 * lines where the page changed, comments and chapters. They are not controls: the lists beside the replay
 * name each one.
 */
function Ticks({ marks, origin, duration }: { marks: readonly ReplayMark[] | undefined; origin: number; duration: number }) {
  if (!marks || marks.length === 0 || duration <= 0) return null;

  return (
    // The thumb's centre runs from half its width in from either end of the track.
    <span aria-hidden data-replay-ticks="" className="pointer-events-none absolute inset-x-[5px] top-1/2 h-0">
      {marks.map((mark, index) => {
        const share = (mark.at - origin) / duration;

        if (share < 0 || share > 1) return null;

        return (
          <span
            key={`${mark.kind}-${index}`}
            data-replay-tick={mark.kind}
            className={`absolute top-0 w-[2px] -translate-x-1/2 -translate-y-1/2 rounded-[1px] ${TICK[mark.kind]}`}
            style={{ left: `${share * 100}%` }}
          />
        );
      })}
    </span>
  );
}

function blocksReplaySpaceShortcut(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  if (target.closest('input[type="range"]')) return false;
  return Boolean(
    target.closest(
      'button, a[href], input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="button"], [role="link"], [role="textbox"], [role="combobox"], [role="menuitem"], [role="slider"]',
    ),
  );
}

function ReplayRequestState({ loading, error }: { loading?: boolean; error?: unknown }) {
  const { t } = useTranslation('rum');
  return (
    <div className="overflow-hidden rounded-lg border border-bd-1 bg-bg-1">
      <div className="flex min-h-11 items-center border-b border-bd-0 px-4 py-2">
        <span className="text-sm font-display text-tx-0">
          {t('session_detail.replay_player')}
        </span>
      </div>
      <div className="grid min-h-80 place-items-center bg-bg-3 p-8 text-center">
        <div>
          {loading ? (
            <LoaderCircle className="mx-auto h-6 w-6 animate-spin text-indigo motion-reduce:animate-none" />
          ) : (
            <AlertTriangle className="mx-auto h-6 w-6 text-red-soft" />
          )}
          <p className="mt-3 text-sm font-strong text-tx-0">
            {t(
              loading
                ? 'session_detail.replay_loading'
                : 'session_detail.replay_request_error',
            )}
          </p>
          {!loading && error != null && (
            <p className="mt-1 max-w-lg break-words font-mono text-xs text-tx-3">
              {error instanceof Error ? error.message : String(error)}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

function TimelineFallback({
  events,
  durationNs,
}: {
  events: PlayerEvent[];
  durationNs?: number | undefined;
}) {
  const { t } = useTranslation('rum');
  return (
    <div className="overflow-hidden rounded-lg border border-bd-1 bg-bg-1">
      <div className="flex min-h-11 flex-wrap items-center gap-3 border-b border-bd-0 px-4 py-2">
        <span className="text-sm font-display text-tx-0">
          {t('session_detail.replay_player')}
        </span>
        <Pill tone="dim">{t('session_detail.action_timeline_fallback')}</Pill>
        <span className="ml-auto text-xs text-tx-3">
          {events.length} {t('session_detail.events_unit')} · {formatDurationNs(durationNs)}
        </span>
      </div>
      <div className="grid min-h-80 place-items-center bg-bg-3 p-8 text-center">
        <div className="max-w-lg">
          <p className="m-0 text-sm font-strong text-tx-0">
            {t('session_detail.replay_unavailable')}
          </p>
          <p className="mb-0 mt-2 text-xs leading-relaxed text-tx-2">
            {t('session_detail.replay_unavailable_description')}
          </p>
        </div>
      </div>
    </div>
  );
}

function validRrwebEvents(events: ReplayEvent[]): RrwebEvent[] {
  return events
    .filter(
      (event): event is ReplayEvent & { type: number; timestamp: number } =>
        typeof event.type === 'number' &&
        Number.isInteger(event.type) &&
        event.type >= 0 &&
        event.type <= 7 &&
        typeof event.timestamp === 'number' &&
        Number.isFinite(event.timestamp),
    )
    .map((event) => ({ ...event, data: event.data ?? {} }))
    .sort((left, right) => left.timestamp - right.timestamp);
}

function viewport(events: RrwebEvent[]): { width: number; height: number } | undefined {
  const meta = events.find((event) => event.type === 4);
  const data = meta?.data;
  if (!data || typeof data !== 'object') return undefined;
  const width = Number((data as Record<string, unknown>).width);
  const height = Number((data as Record<string, unknown>).height);
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    return undefined;
  }
  return { width, height };
}

function formatReplayTime(milliseconds: number): string {
  const seconds = Math.max(0, Math.floor(milliseconds / 1_000));
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
}
