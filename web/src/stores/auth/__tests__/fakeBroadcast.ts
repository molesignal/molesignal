import type { ChannelLike } from '../tabSession';

type Listener = (event: MessageEvent) => void;

/** One browser: every channel opened on it hears what the others post. */
export class FakeNetwork {
  private readonly channels = new Set<FakeChannel>();

  open(): FakeChannel {
    const channel = new FakeChannel(this.channels);
    this.channels.add(channel);
    return channel;
  }
}

export class FakeChannel implements ChannelLike {
  private readonly listeners = new Set<Listener>();

  constructor(private readonly peers: Set<FakeChannel>) {}

  postMessage(message: unknown): void {
    for (const peer of this.peers) {
      if (peer === this) continue;
      globalThis.queueMicrotask(() => {
        for (const listener of peer.listeners) {
          listener({ data: globalThis.structuredClone(message) } as MessageEvent);
        }
      });
    }
  }

  addEventListener(_type: 'message', listener: Listener): void {
    this.listeners.add(listener);
  }

  removeEventListener(_type: 'message', listener: Listener): void {
    this.listeners.delete(listener);
  }
}
