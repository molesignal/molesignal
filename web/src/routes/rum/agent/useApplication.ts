import { useQuery } from '@tanstack/react-query';

import * as rumApi from '@/api/rum';

/**
 * An application as the person knows it: its name, or its id until the list of applications is
 * read. The list is the one the pages share, so it costs nothing where a page has it already. With
 * no id (an incident that is not about an application) nothing is read.
 */
export function useApplicationRef(id: string): { id: string; name: string } {
  const applications = useQuery({
    queryKey: ['rum', 'applications'],
    queryFn: () => rumApi.listApplications(),
    enabled: id !== '',
  });

  return { id, name: applications.data?.find((application) => application.id === id)?.name ?? id };
}
