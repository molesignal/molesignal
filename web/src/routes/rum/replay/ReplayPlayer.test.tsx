import { cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { ReplayEvent, SessionRow } from '@/api/rum';

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

function renderPlayer() {
  return render(
    <ReplayPlayer
      replayEvents={EVENTS}
      timelineEvents={[]}
      session={{} as SessionRow}
    />,
  );
}
