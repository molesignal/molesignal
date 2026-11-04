import * as React from 'react';

import type { QueryEditorMode } from '@/shell/query/ModeSwitch';
import { useQueryTabState } from '@/shell/queryTabs';

/**
 * The query being written on the explorer, before it is run.
 *
 * `editorMode` is how it is written: as a statement (Code) or put together from filters
 * (Builder). Both write the one field query, so the draft is one text.
 */
export function useRumQueryDraft(initial: string) {
  const [editorMode, setEditorMode] = useQueryTabState<QueryEditorMode>('rum.editorMode', 'code');
  // A field picked in the field list while the builder is open starts a filter there.
  const [builderField, setBuilderField] = React.useState<string | null>(null);
  const [queryDraft, setQueryDraft] = React.useState(initial);

  return { editorMode, setEditorMode, builderField, setBuilderField, queryDraft, setQueryDraft };
}
