import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as connectorsApi from '@/api/connectors';
import * as streamsApi from '@/api/streams';

import { isDestinationKind } from '../connectors/kinds';

/**
 * What a pipeline can read from and write to in this workspace: its streams
 * and the connectors it can deliver to. Loaded once for the whole editor, so
 * the flow, the fields and the validation agree on what exists.
 */
export function usePipelineResources() {
  const streamsQuery = useQuery({
    queryKey: ['streams', 'pipeline-options'],
    queryFn: () => streamsApi.list(500),
  });
  const connectorsQuery = useQuery({
    queryKey: ['connectors'],
    queryFn: () => connectorsApi.list(),
  });

  return React.useMemo(() => {
    // A connector that brings data in is not somewhere to deliver it.
    const connectors = (connectorsQuery.data ?? []).filter((connector) => isDestinationKind(connector.kind));

    return {
      streams: streamsQuery.data ?? [],
      streamsPending: streamsQuery.isPending,
      streamsFailed: streamsQuery.isError,
      connectors,
      connectorIds: connectors.map((connector) => connector.id),
      connectorNames: Object.fromEntries(
        connectors.map((connector) => [connector.id, connector.name]),
      ) as Record<string, string>,
    };
  }, [connectorsQuery.data, streamsQuery.data, streamsQuery.isError, streamsQuery.isPending]);
}

export type PipelineResources = ReturnType<typeof usePipelineResources>;

/** The names connectors go by, for showing a saved pipeline's destinations. */
export function useConnectorNames(): Record<string, string> {
  const connectorsQuery = useQuery({
    queryKey: ['connectors'],
    queryFn: () => connectorsApi.list(),
  });

  return React.useMemo(
    () =>
      Object.fromEntries(
        (connectorsQuery.data ?? []).map((connector) => [connector.id, connector.name]),
      ),
    [connectorsQuery.data],
  );
}
