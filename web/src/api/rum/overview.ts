import { whenStreamExists } from '@/api/missingStream';
import type { ExperienceGrade } from '@/api/rum';
import { http } from '@/lib/http';

export interface OverviewParams {
  org_id: string;
  from_micros: number;
  to_micros: number;
  application?: string;
  environment?: string;
  version?: string;
  country?: string;
  device?: string;
  summary_only?: boolean;
}

export interface OverviewMetrics {
  users: number;
  sessions: number;
  errorFreeRate: number;
  lcpP75: number;
  inpP75: number;
  clsP75: number;
}

export interface ExperienceBucket {
  start: number;
  good: number;
  needs: number;
  poor: number;
}

export interface SatisfactionCounts {
  good: number;
  needsImprovement: number;
  poor: number;
  total: number;
}

export interface OverviewSlowPage {
  page: string;
  p75: number;
  sessions: number;
  errorRate: number;
  grade: ExperienceGrade;
}

export interface OverviewFrequentError {
  fingerprint: string;
  message: string;
  users: number;
  version?: string;
}

export interface OverviewDimensionShare {
  label: string;
  count: number;
  share: number;
}

export interface OverviewFacets {
  applications: string[];
  environments: string[];
  versions: string[];
  countries: string[];
  devices: string[];
}

export interface OverviewSummaryData {
  metrics: OverviewMetrics;
  browserDevices: OverviewDimensionShare[];
  regions: OverviewDimensionShare[];
  facets: OverviewFacets;
}

export interface OverviewInsightsData {
  trend: ExperienceBucket[];
  satisfaction: SatisfactionCounts;
  slowPages: OverviewSlowPage[];
  frequentErrors: OverviewFrequentError[];
}

/** The overview of a workspace no session has reached yet. */
function emptyOverview(): OverviewSummaryData {
  return {
    metrics: {
      users: 0,
      sessions: 0,
      errorFreeRate: 0,
      lcpP75: 0,
      inpP75: 0,
      clsP75: 0,
    },
    browserDevices: [],
    regions: [],
    facets: {
      applications: [],
      environments: [],
      versions: [],
      countries: [],
      devices: [],
    },
  };
}

function emptyInsights(): OverviewInsightsData {
  return {
    trend: [],
    satisfaction: { good: 0, needsImprovement: 0, poor: 0, total: 0 },
    slowPages: [],
    frequentErrors: [],
  };
}

/**
 * Fetches only the first-paint aggregates; it never returns raw RUM rows.
 * Before the first session arrives there is no stream to read, which is an
 * empty overview rather than a failure.
 */
export async function getOverview(
  params: OverviewParams,
): Promise<OverviewSummaryData> {
  return whenStreamExists(async () => {
    const { data } = await http.get<OverviewSummaryData>('/rum/overview', {
      params: {
        from: params.from_micros,
        to: params.to_micros,
        ...(params.application ? { application: params.application } : {}),
        ...(params.environment ? { environment: params.environment } : {}),
        ...(params.version ? { version: params.version } : {}),
        ...(params.country ? { country: params.country } : {}),
        ...(params.device ? { device: params.device } : {}),
        ...(params.summary_only ? { summary_only: true } : {}),
      },
    });
    return data;
  }, emptyOverview);
}

/** Fetches the below-the-fold analysis after the overview summary is visible. */
export async function getOverviewInsights(
  params: OverviewParams,
): Promise<OverviewInsightsData> {
  return whenStreamExists(async () => {
    const { data } = await http.get<OverviewInsightsData>(
      '/rum/overview/insights',
      {
        params: {
          from: params.from_micros,
          to: params.to_micros,
          ...(params.application ? { application: params.application } : {}),
          ...(params.environment ? { environment: params.environment } : {}),
          ...(params.version ? { version: params.version } : {}),
          ...(params.country ? { country: params.country } : {}),
          ...(params.device ? { device: params.device } : {}),
        },
      },
    );
    return data;
  }, emptyInsights);
}
