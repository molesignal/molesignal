import * as React from 'react';

import { type OpenView } from './openView';
import { RumDetailDrawer, type RumDrawerState } from '../explorer/Drawer';
import { useRumSchema } from '../explorer/useRumSchema';

/** How long before the thing that led here a view is looked for: a day, which no page load outlasts. */
const LOOKBACK_MICROS = 24 * 60 * 60 * 1_000_000;
const AFTER_MICROS = 60 * 1_000_000;

/**
 * The drawer of the explorer, opened on a view by a page of its own: the waterfall of its load is in
 * front, with its fields beside it. Closing it lets go of the view in the address.
 */
export function ViewDrawerHost({ open, onClose }: { open: OpenView | null; onClose: () => void }) {
  const schema = useRumSchema('views');
  const returnFocus = React.useRef<HTMLElement | null>(null);

  const state = React.useMemo<RumDrawerState | null>(() => {
    if (!open) return null;

    const at = open.at ?? Date.now() * 1_000;

    return {
      tab: 'views',
      row: { timestamp: at, 'view.id': open.id },
      open: true,
      span: { from: at - LOOKBACK_MICROS, to: at + AFTER_MICROS },
      focus: open.focus || undefined,
      focusAt: open.at ?? undefined,
    };
  }, [open]);

  return <RumDetailDrawer state={state} fields={schema.data?.fields ?? []} onClose={onClose} neighbours={{}} returnFocusRef={returnFocus} />;
}
