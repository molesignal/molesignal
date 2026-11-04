import { describe, expect, it, vi } from 'vitest';

import { ReplayController, replayClockMicros } from './controller';

describe('what a page knows of a replay', () => {
  it('knows nothing until a player says what it knows', () => {
    const controller = new ReplayController();

    expect(controller.getState()).toEqual({ origin: null, duration: 0, position: 0, playing: false, seekable: false });
    expect(replayClockMicros(controller.getState())).toBeNull();
  });

  it('tells its listeners when what the player says changes, and not when it says the same again', () => {
    const controller = new ReplayController();
    const listener = vi.fn();

    controller.subscribe(listener);
    controller.publish({ origin: 1_000, duration: 60_000, position: 0 });
    controller.publish({ origin: 1_000, duration: 60_000, position: 0 });
    controller.publish({ position: 2_500, playing: true });

    expect(listener).toHaveBeenCalledTimes(2);
    expect(controller.getState()).toMatchObject({ origin: 1_000, duration: 60_000, position: 2_500, playing: true });
  });

  it('stops telling a listener that has gone', () => {
    const controller = new ReplayController();
    const listener = vi.fn();

    controller.subscribe(listener)();
    controller.publish({ position: 10 });

    expect(listener).not.toHaveBeenCalled();
  });

  it('says the moment of the replay as the rows of a session do: microseconds since the epoch', () => {
    const controller = new ReplayController();

    controller.publish({ origin: 1_760_000_000_000, position: 5_000 });

    expect(replayClockMicros(controller.getState())).toBe(1_760_000_005_000_000);
  });
});

describe('moving a replay', () => {
  it('moves nothing where there is no player', () => {
    const controller = new ReplayController();

    expect(controller.seek(1_000)).toBe(false);
    expect(controller.seekToMicros(1_000_000)).toBe(false);
  });

  it('moves the player to a moment that lies in the recording, and holds a moment outside it at its edge', () => {
    const controller = new ReplayController();
    const seeker = vi.fn();

    controller.publish({ origin: 1_000, duration: 60_000 });
    controller.attach(seeker);

    expect(controller.getState().seekable).toBe(true);
    expect(controller.seek(12_000)).toBe(true);
    expect(controller.seek(-5)).toBe(true);
    expect(controller.seek(90_000)).toBe(true);
    expect(seeker.mock.calls.map(([moment]) => moment)).toEqual([12_000, 0, 60_000]);
  });

  it('moves it to a moment of the clock of the rows, by where the recording began', () => {
    const controller = new ReplayController();
    const seeker = vi.fn();

    controller.publish({ origin: 1_760_000_000_000, duration: 60_000 });
    controller.attach(seeker);

    expect(controller.seekToMicros(1_760_000_007_500_000)).toBe(true);
    expect(seeker).toHaveBeenCalledWith(7_500);
  });

  it('forgets everything when the player is taken away', () => {
    const controller = new ReplayController();

    controller.publish({ origin: 1_000, duration: 60_000, position: 4_000, playing: true });
    controller.attach(() => undefined);
    controller.attach(null);

    expect(controller.getState()).toEqual({ origin: null, duration: 0, position: 0, playing: false, seekable: false });
    expect(controller.seek(1)).toBe(false);
  });
});
