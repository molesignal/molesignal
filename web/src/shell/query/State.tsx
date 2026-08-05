import { AlertTriangle, Database, Loader2 } from 'lucide-react';
import * as React from 'react';

import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';

/**
 * Small fixed-height block used inside page bodies when a backend query is
 * pending / empty / errored. Pages keep their chrome (PageHeader, KPI strip)
 * and slot one of these into the data region — never silently fall back to
 * sample data.
 */
export function QueryState({
  state,
  error,
  empty,
  loadingLabel = 'Loading…',
  emptyLabel = 'No data',
  errorLabel,
  className,
}: {
  state: 'loading' | 'empty' | 'error';
  error?: unknown;
  empty?: React.ReactNode;
  loadingLabel?: string;
  emptyLabel?: React.ReactNode;
  errorLabel?: React.ReactNode;
  className?: string;
}) {
  const Icon = state === 'loading' ? Loader2 : state === 'empty' ? Database : AlertTriangle;
  const content =
    state === 'loading'
      ? loadingLabel
      : state === 'empty'
        ? (empty ?? emptyLabel)
        : errorLabel ??
          (error
          ? toApiError(error).message
          : 'Request failed');

  return (
    <div
      role={state === 'error' ? 'alert' : 'status'}
      className={cn(
        'flex min-h-40 flex-1 flex-col items-center justify-center gap-3 px-6 py-8 text-center font-sans',
        className,
      )}
    >
      <div className="grid h-10 w-10 place-items-center rounded-lg border border-bd-0 bg-bg-2">
        <Icon
          className={cn(
            'h-5 w-5',
            state === 'loading' && 'animate-spin text-blue',
            state === 'empty' && 'text-tx-3',
            state === 'error' && 'text-red-soft',
          )}
        />
      </div>
      <div
        className={cn(
          'max-w-xl break-words text-sm leading-relaxed',
          state === 'error' ? 'text-red-soft' : 'text-tx-2',
        )}
      >
        {content}
      </div>
    </div>
  );
}

/**
 * Convenience reducer: maps react-query's `{ isLoading, isError, data }`
 * into the QueryState `state` discriminator. Returns `null` when data is
 * ready so the caller renders the real list.
 */
export function queryStateFor(args: {
  isLoading: boolean;
  isError: boolean;
  data: unknown;
}): 'loading' | 'empty' | 'error' | null {
  if (args.isLoading) return 'loading';
  if (args.isError) return 'error';
  const d = args.data;
  if (d == null) return 'empty';
  if (Array.isArray(d) && d.length === 0) return 'empty';
  return null;
}
