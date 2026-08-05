import { nanoid } from 'nanoid';
import * as React from 'react';
import type uPlot from 'uplot';

import { useCursorChannel } from '@/time/CursorChannel';

/**
 * Wire a uPlot instance to a shared CursorChannel scoped by `scopeId`.
 *
 * - When the uPlot cursor moves, publish `t` with this plot's own source id
 *   so it does not echo back into itself.
 * - When another publisher pushes a `t`, project it to x-pixels and call
 *   `setCursor({ left }, false)` with `false` to suppress re-publish.
 */
export function useCursorSync(
  plotRef: React.MutableRefObject<uPlot | null>,
  scopeId: string,
  enabled = true,
) {
  const sourceId = React.useMemo(() => nanoid(8), []);
  const channel = useCursorChannel(scopeId);

  React.useEffect(() => {
    if (!enabled) {
      plotRef.current?.setCursor({ left: -1, top: -1 }, false);
      return;
    }
    return channel.subscribe((t, sender) => {
      if (sender === sourceId) return;
      const plot = plotRef.current;
      if (!plot) return;
      const x = plot.valToPos(t, 'x');
      plot.setCursor({ left: x, top: plot.cursor.top ?? -1 }, false);
    });
  }, [channel, enabled, sourceId, plotRef]);

  const onCursorMove = React.useCallback(
    (t: number) => {
      if (enabled) channel.publish(t, sourceId);
    },
    [channel, enabled, sourceId],
  );

  return { onCursorMove, sourceId };
}
