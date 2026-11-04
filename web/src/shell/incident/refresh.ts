import type { useQueryClient } from '@tanstack/react-query';

/** Drops every cached copy of incidents so lists, details and timelines read them again. */
export async function refreshIncidents(
  queryClient: ReturnType<typeof useQueryClient>,
  id?: string,
) {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ['alerts', 'incidents'] }),
    queryClient.invalidateQueries({ queryKey: ['incidents'] }),
    id
      ? queryClient.invalidateQueries({ queryKey: ['incident-timeline', id] })
      : Promise.resolve(),
  ]);
}
