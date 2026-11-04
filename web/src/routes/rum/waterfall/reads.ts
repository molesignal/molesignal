import { useQuery } from '@tanstack/react-query';

import * as rumApi from '@/api/rum';
import { useAuthStore } from '@/stores/auth';

import { resourcesQuery, tasksQuery, type ViewFacts } from './model';

/**
 * What a view asked for and what blocked it: its resources and its long tasks, each read once, by the
 * id of the view and the frame of its load. A view that is not known yet reads nothing.
 */
export function useViewReads(view: ViewFacts | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const enabled = Boolean(orgId && view?.id);

  const resources = useQuery({
    queryKey: ['rum', 'waterfall', orgId, 'resources', view?.id, view?.start],
    queryFn: () => rumApi.search(resourcesQuery(view as ViewFacts)),
    enabled,
    staleTime: 60_000,
  });

  const tasks = useQuery({
    queryKey: ['rum', 'waterfall', orgId, 'tasks', view?.id, view?.start],
    queryFn: () => rumApi.search(tasksQuery(view as ViewFacts)),
    enabled,
    staleTime: 60_000,
  });

  return { resources, tasks };
}
