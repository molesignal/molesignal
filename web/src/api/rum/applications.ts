import { http } from '@/lib/http';

export interface ApplicationSummaryParams {
  org_id: string;
  from_micros: number;
  to_micros: number;
  environment?: string;
  version?: string;
}

export interface ApplicationSummary {
  application: string;
  environments: string[];
  versions: string[];
  users: number;
  sessions: number;
  errorFreeRate: number;
  lcpP75: number;
}

interface ApplicationSummaryResponse {
  items: ApplicationSummary[];
}

/** Fetches the compact application read model; no session or action rows leave the backend. */
export async function listApplicationSummaries(
  params: ApplicationSummaryParams,
): Promise<ApplicationSummary[]> {
  const { data } = await http.get<ApplicationSummaryResponse>(
    '/rum/applications/summary',
    {
      params: {
        from: params.from_micros,
        to: params.to_micros,
        ...(params.environment ? { environment: params.environment } : {}),
        ...(params.version ? { version: params.version } : {}),
      },
    },
  );
  return data.items;
}
