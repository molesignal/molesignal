import * as React from 'react';

import { UnsavedPrompt } from './UnsavedPrompt';
import { useDraftTouched } from './useDraftTouched';

export const DraftChangeContext = React.createContext<(() => void) | null>(null);

function noop() {}

/** Says a draft changed where no field does: a row added, moved or removed, a card chosen. */
export function useMarkDraftChanged(): () => void {
  return React.useContext(DraftChangeContext) ?? noop;
}

/**
 * The draft a form drawer holds. Once something in it changed, closing it asks
 * first. `dirty` overrides "something was touched" where the form knows better.
 */
export function useDrawerDraft({
  open,
  holdsDraft,
  dirty,
  onOpenChange,
}: {
  open: boolean;
  holdsDraft: boolean;
  dirty: boolean | undefined;
  onOpenChange: (open: boolean) => void;
}) {
  const { touched, markChanged, reset, events } = useDraftTouched();
  const [asking, setAsking] = React.useState(false);

  // A new opening is a new draft.
  React.useEffect(() => {
    if (!open) return;

    reset();
    setAsking(false);
  }, [open, reset]);

  return {
    markChanged,
    contentEvents: events,
    requestOpenChange: (next: boolean) => {
      if (!next && holdsDraft && (dirty ?? touched)) {
        setAsking(true);
        return;
      }
      onOpenChange(next);
    },
    prompt: (
      <UnsavedPrompt
        open={asking}
        kind="discard"
        onKeep={() => setAsking(false)}
        onDiscard={() => {
          setAsking(false);
          onOpenChange(false);
        }}
      />
    ),
  };
}
