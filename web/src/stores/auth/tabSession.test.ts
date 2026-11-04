import { beforeEach, describe, expect, it, vi } from 'vitest';

import { FakeNetwork } from './__tests__/fakeBroadcast';
import { createTabSessionSharing } from './tabSession';

let network: FakeNetwork;
const tab = () => createTabSessionSharing({ openChannel: () => network.open() });

/** Lets the channel deliver what was posted. */
const delivered = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  network = new FakeNetwork();
});

describe('announcing to the other tabs', () => {
  it('tells every other tab about a sign-out, not the tab itself', async () => {
    const here = tab();
    const there = { onSignOut: vi.fn(), onSession: vi.fn() };
    const self = { onSignOut: vi.fn(), onSession: vi.fn() };
    tab().listen(there);
    here.listen(self);

    here.announceSignOut();
    await delivered();

    expect(there.onSignOut).toHaveBeenCalledTimes(1);
    expect(self.onSignOut).not.toHaveBeenCalled();
  });

  it('tells the other tabs which session the browser now holds', async () => {
    const there = { onSignOut: vi.fn(), onSession: vi.fn() };
    tab().listen(there);

    tab().announceSession('u1:org-2');
    await delivered();

    expect(there.onSession).toHaveBeenCalledWith('u1:org-2');
    expect(there.onSignOut).not.toHaveBeenCalled();
  });

  it('stops hearing once the listener is stopped', async () => {
    const there = { onSignOut: vi.fn(), onSession: vi.fn() };
    const stop = tab().listen(there);
    stop();

    tab().announceSignOut();
    await delivered();

    expect(there.onSignOut).not.toHaveBeenCalled();
  });

  it('ignores messages it does not understand', async () => {
    const there = { onSignOut: vi.fn(), onSession: vi.fn() };
    tab().listen(there);

    network.open().postMessage({ type: 'hello', from: 'tab' });
    network.open().postMessage({ type: 'session', from: 'tab', session: 42 });
    await delivered();

    expect(there.onSession).not.toHaveBeenCalled();
    expect(there.onSignOut).not.toHaveBeenCalled();
  });
});

describe('where BroadcastChannel does not exist', () => {
  it('listens to and announces nothing, without failing', () => {
    const sharing = createTabSessionSharing({ openChannel: () => null });
    const stop = sharing.listen({ onSignOut: vi.fn(), onSession: vi.fn() });

    expect(() => sharing.announceSignOut()).not.toThrow();
    expect(() => sharing.announceSession('u1:org-1')).not.toThrow();
    expect(() => stop()).not.toThrow();
  });
});
