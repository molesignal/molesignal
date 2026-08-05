import { Sparkles } from 'lucide-react';
import * as React from 'react';
import { useNavigate } from 'react-router-dom';

import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/shell/ui/context-menu';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import { useTimeStore } from '@/stores/useTimeStore';

import { providersFor, type LinkProvider, type SourceCtx } from './providers';
import { fetchServerCorrelation } from './server';

interface BaseProps {
  source: Omit<SourceCtx, 'globalWindow'>;
  /**
   * Push behaviour: 'replace' updates the top frame, 'append_layer' stacks on
   * top of the parent frame. Defaults to append_layer because correlation
   * jumps are usually deepening the investigation.
   */
  mode?: 'append_layer' | 'new_stack';
  parentFrameId?: string;
}

/**
 * Right-click anywhere within this trigger to expose the "View related" menu
 * filtered by the source's `kind`.
 */
export function CorrelationContextMenuTrigger({
  source,
  mode = 'append_layer',
  parentFrameId,
  children,
}: BaseProps & { children: React.ReactNode }) {
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuLabel>View related</ContextMenuLabel>
        <ContextMenuSeparator />
        <CorrelationItems source={source} mode={mode} {...(parentFrameId && { parentFrameId })} renderAs="context" />
      </ContextMenuContent>
    </ContextMenu>
  );
}

/**
 * The dropdown variant, used when "View related" lives behind an explicit
 * affordance like a button next to a field.
 */
export function CorrelationDropdown({
  source,
  mode = 'append_layer',
  parentFrameId,
  children,
}: BaseProps & { children: React.ReactNode }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
      <DropdownMenuContent>
        <DropdownMenuLabel>View related</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <CorrelationItems source={source} mode={mode} {...(parentFrameId && { parentFrameId })} renderAs="dropdown" />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function CorrelationItems({
  source,
  mode,
  parentFrameId,
  renderAs,
}: BaseProps & { renderAs: 'context' | 'dropdown' }) {
  const providers = providersFor(source.kind);
  const nav = useNavigate();
  const stack = useInvestigationStack();
  const globalWindow = useTimeStore((s) => s.window);

  if (providers.length === 0) {
    const Empty = renderAs === 'context' ? ContextMenuItem : DropdownMenuItem;
    return <Empty disabled>No related signals</Empty>;
  }

  const onPick = async (p: LinkProvider) => {
    // Try server-side derivation first; fall back to local.
    const ctx = { from_kind: source.kind, fields: source.fields, at: source.t };
    const server = await fetchServerCorrelation(p.from, p.to, ctx);
    const derived = server ?? p.derive({ ...source, globalWindow });

    if (mode === 'new_stack') stack.reset();
    stack.push({
      kind: p.targetFrameKind,
      params: { filters: derived.filters, prefill: derived.prefill ?? {} },
      time_range_override: {
        from: derived.time_range.from,
        to: derived.time_range.to,
        mode: 'absolute',
      },
      ...(parentFrameId && { parent_frame_id: parentFrameId }),
    });
    nav('/investigate');
  };

  const Item = renderAs === 'context' ? ContextMenuItem : DropdownMenuItem;

  return (
    <>
      {providers.map((p) => (
        <Item key={`${p.from}-${p.to}`} onSelect={() => void onPick(p)}>
          <Sparkles className="h-4 w-4 opacity-70" />
          {p.label}
        </Item>
      ))}
    </>
  );
}
