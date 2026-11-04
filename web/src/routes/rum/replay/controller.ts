import * as React from 'react';

/** Where a replay is, as its player says it. */
export interface ReplayState {
  /** When the first event of the recording happened, in milliseconds since the epoch; none until a player has read it. */
  origin: number | null;
  /** How long the recording lasts, in milliseconds. */
  duration: number;
  /** How far into the recording the replay has got, in milliseconds. */
  position: number;
  playing: boolean;
  /** A player is there to be moved. */
  seekable: boolean;
}

const IDLE: ReplayState = { origin: null, duration: 0, position: 0, playing: false, seekable: false };

const same = (a: ReplayState, b: ReplayState) =>
  a.origin === b.origin && a.duration === b.duration && a.position === b.position && a.playing === b.playing && a.seekable === b.seekable;

/**
 * What the parts of a page beside a replay know of it, and how they move it. The player says where it
 * is as it plays and gives the way to move it while it exists; a list of what happened in the session
 * reads the place, to light the row that is under the replay, and moves the replay to the row a person
 * chose. The parts do not hold the player, so none of them needs to be inside it.
 */
export class ReplayController {
  private state: ReplayState = IDLE;
  private seeker: ((milliseconds: number) => void) | null = null;
  private readonly listeners = new Set<() => void>();

  getState = (): ReplayState => this.state;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);

    return () => {
      this.listeners.delete(listener);
    };
  };

  private set(next: ReplayState) {
    if (same(this.state, next)) return;

    this.state = next;
    this.listeners.forEach((listener) => listener());
  }

  /** For the player: what it knows now. */
  publish(known: Partial<Omit<ReplayState, 'seekable'>>) {
    this.set({ ...this.state, ...known });
  }

  /** For the player: the way to move it, until it is taken away again, which is the end of what it knew. */
  attach(seeker: ((milliseconds: number) => void) | null) {
    this.seeker = seeker;
    this.set(seeker ? { ...this.state, seekable: true } : IDLE);
  }

  /** Moves the replay to a moment, in milliseconds from its beginning, and holds it there. Nothing moves where there is no player. */
  seek(milliseconds: number): boolean {
    if (!this.seeker) return false;

    this.seeker(Math.min(Math.max(0, milliseconds), this.state.duration));

    return true;
  }

  /** The same for a moment as the rows of a session say it: microseconds since the epoch. */
  seekToMicros(micros: number): boolean {
    const { origin } = this.state;

    return origin === null ? false : this.seek(micros / 1_000 - origin);
  }
}

/**
 * Something computed from where the replay is. The part that reads it is drawn again only when the
 * answer changes, so a row that is lit by it is not drawn ten times a second: the answer must be a
 * value that compares equal when it is the same, a number or a text.
 */
export function useReplaySelector<T extends string | number | boolean | null>(controller: ReplayController, select: (state: ReplayState) => T): T {
  return React.useSyncExternalStore(controller.subscribe, () => select(controller.getState()));
}

/** A moment of the replay as the clock of the rows says it: microseconds since the epoch; none until a player has read one. */
export const replayClockMicros = (state: ReplayState): number | null => (state.origin === null ? null : (state.origin + state.position) * 1_000);
