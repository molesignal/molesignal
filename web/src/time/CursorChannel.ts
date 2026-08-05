import mitt, { type Emitter } from 'mitt';
import * as React from 'react';

type Events = {
  cursor: { t: number; sourceId: string };
};

const channels = new Map<string, Emitter<Events>>();

function getOrCreate(scopeId: string): Emitter<Events> {
  let e = channels.get(scopeId);
  if (!e) {
    e = mitt<Events>();
    channels.set(scopeId, e);
  }
  return e;
}

export function publishCursor(scopeId: string, t: number, sourceId: string) {
  getOrCreate(scopeId).emit('cursor', { t, sourceId });
}

export interface CursorChannel {
  subscribe: (cb: (t: number, sourceId: string) => void) => () => void;
  publish: (t: number, sourceId: string) => void;
}

export function useCursorChannel(scopeId: string): CursorChannel {
  const channel = React.useMemo(() => getOrCreate(scopeId), [scopeId]);

  return React.useMemo(
    () => ({
      subscribe: (cb) => {
        const handler = (ev: { t: number; sourceId: string }) => cb(ev.t, ev.sourceId);
        channel.on('cursor', handler);
        return () => channel.off('cursor', handler);
      },
      publish: (t, sourceId) => channel.emit('cursor', { t, sourceId }),
    }),
    [channel],
  );
}
