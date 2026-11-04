import { act, cleanup, fireEvent, render } from '@testing-library/react';
import type * as React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { ReplayEvent } from '@/api/rum';

import { ReplayController } from './controller';
import { ReplayPlayer } from './ReplayPlayer';

const replayer = vi.hoisted(() => ({
  play: vi.fn(),
  pause: vi.fn(),
  setConfig: vi.fn(),
  destroy: vi.fn(),
  disableInteract: vi.fn(),
}));

vi.mock('@rrweb/replay', () => ({
  Replayer: class {
    wrapper = document.createElement('div');

    disableInteract = replayer.disableInteract;
    destroy = replayer.destroy;
    pause = replayer.pause;
    play = replayer.play;
    setConfig = replayer.setConfig;

    getMetaData() {
      return { totalTime: 48_000 };
    }

    getCurrentTime() {
      return 6_000;
    }

    on() {}
  },
}));

const EVENTS: ReplayEvent[] = [
  { type: 4, timestamp: 1_000, data: { width: 1_440, height: 900 } },
  { type: 2, timestamp: 1_001, data: {} },
];

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('ReplayPlayer keyboard controls', () => {
  it('uses Space to toggle playback, including when the seek control has focus', () => {
    const { container } = renderPlayer();

    expect(fireEvent.keyDown(window, { code: 'Space', key: ' ' })).toBe(false);
    expect(replayer.play).toHaveBeenCalledTimes(1);

    const seek = container.querySelector<HTMLInputElement>('input[type="range"]');
    expect(seek).not.toBeNull();
    expect(fireEvent.keyDown(seek!, { code: 'Space', key: ' ' })).toBe(false);
    expect(replayer.pause).toHaveBeenCalledTimes(1);
  });

  it('does not hijack Space from text input', () => {
    const { container } = renderPlayer();
    const input = document.createElement('input');
    container.append(input);

    expect(fireEvent.keyDown(input, { code: 'Space', key: ' ' })).toBe(true);
    expect(replayer.play).not.toHaveBeenCalled();
  });

  it('renders a compact ten-pixel seek thumb', () => {
    const { container } = renderPlayer();
    const seek = container.querySelector<HTMLInputElement>('input[type="range"]');

    expect(seek?.className).toContain('[&::-webkit-slider-thumb]:h-2.5');
    expect(seek?.className).toContain('[&::-moz-range-thumb]:h-2.5');
  });
});

describe('ReplayPlayer beside a page', () => {
  it('tells the controller where the recording began, how long it is, and gives it the way to move the replay', () => {
    const controller = new ReplayController();

    renderPlayer({ controller });

    expect(controller.getState()).toMatchObject({ origin: 1_000, duration: 48_000, position: 0, playing: false, seekable: true });

    let moved = false;

    act(() => {
      moved = controller.seek(12_500);
    });

    expect(moved).toBe(true);
    expect(replayer.pause).toHaveBeenCalledWith(12_500);
    expect(controller.getState().position).toBe(12_500);
  });

  it('lets go of the controller when it goes', () => {
    const controller = new ReplayController();
    const { unmount } = renderPlayer({ controller });

    unmount();

    expect(controller.getState().seekable).toBe(false);
    expect(controller.seek(1)).toBe(false);
  });

  it('opens at the moment it is asked to, once, and waits there', () => {
    const { rerender } = renderPlayer({ startAt: 7_000 });

    expect(replayer.pause).toHaveBeenCalledTimes(1);
    expect(replayer.pause).toHaveBeenCalledWith(6_000);
    expect(replayer.play).not.toHaveBeenCalled();

    rerender(<ReplayPlayer replayEvents={EVENTS} timelineEvents={[]} startAt={7_000} />);

    expect(replayer.pause).toHaveBeenCalledTimes(1);
  });

  it('opens at the beginning of the recording for a moment before it, and at its end for one after it', () => {
    renderPlayer({ startAt: 500 });
    expect(replayer.pause).toHaveBeenLastCalledWith(0);

    cleanup();
    renderPlayer({ startAt: 1_000_000 });
    expect(replayer.pause).toHaveBeenLastCalledWith(48_000);
  });

  it('ticks the ruler where an error, a sign of frustration and a new page lie, and leaves out what is outside the recording', () => {
    const { container } = renderPlayer({
      marks: [
        { at: 13_000, kind: 'error' },
        { at: 25_000, kind: 'frustration' },
        { at: 37_000, kind: 'view' },
        { at: 900_000, kind: 'error' },
        { at: 10, kind: 'view' },
      ],
    });

    const ticks = [...container.querySelectorAll<HTMLElement>('[data-replay-tick]')];

    expect(ticks.map((tick) => tick.dataset.replayTick)).toEqual(['error', 'frustration', 'view']);
    // 12 s of the 48 s the recording runs in.
    expect(ticks[0]?.style.left).toBe('25%');
  });
});

function renderPlayer(props: Partial<React.ComponentProps<typeof ReplayPlayer>> = {}) {
  return render(
    <ReplayPlayer replayEvents={EVENTS} timelineEvents={[]} durationNs={48_000_000_000} {...props} />,
  );
}
