import { useQuery } from '@tanstack/react-query';

import * as rumApi from '@/api/rum';

import type { RumTab } from './model';

/** The columns of a type. They are fixed by the server, so they are read once in a while. */
export function useRumSchema(tab: RumTab) {
  return useQuery({
    queryKey: ['rum', 'schema', tab],
    queryFn: ({ signal }) => rumApi.schema(tab, signal),
    staleTime: 10 * 60_000,
  });
}
