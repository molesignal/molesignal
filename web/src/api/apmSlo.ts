import { http } from '@/lib/http';

import type { TransactionIdentity } from './apm';

/** What makes a request good: it did not fail, or it did not fail and was quick enough. */
export type SloKind = 'availability' | 'latency';

export type SloState = 'healthy' | 'at_risk' | 'breached' | 'no_data';

/** The windows an objective can be held over, in days. */
export const SLO_WINDOW_DAYS = [7, 28, 30] as const;

export interface ServiceLevelObjective {
  id: string;
  name: string;
  description: string;
  namespace: string;
  service: string;
  environment: string;
  /** One endpoint of the service; every request of the service when absent. */
  transaction?: TransactionIdentity | null;
  kind: SloKind;
  latencyThreshold?: number | null;
  /** The share of good requests promised, such as 0.999. */
  objective: number;
  windowDays: number;
  createdBy: string;
  createdAt: number;
  updatedBy: string;
  updatedAt: number;
}

export interface SloStatus {
  slo: ServiceLevelObjective;
  state: SloState;
  /** Good requests over all requests in the window. */
  sli?: number | null;
  goodRequests: number;
  totalRequests: number;
  /** The share of the error budget not yet spent; below zero once broken. */
  budgetRemaining?: number | null;
  /** How many times faster than allowed the budget is spent. */
  burnRates: { oneHour?: number | null; sixHours?: number | null; oneDay?: number | null };
}

export interface SloDay {
  dayStart: number;
  sli?: number | null;
  goodRequests: number;
  totalRequests: number;
}

export interface SloDetail {
  status: SloStatus;
  history: SloDay[];
}

export type SloWrite = Omit<ServiceLevelObjective, 'id' | 'createdBy' | 'createdAt' | 'updatedBy' | 'updatedAt'>;

/** A service, as SLOs are listed for one. */
export interface SloServiceFilter {
  namespace: string;
  service: string;
  environment: string;
}

export const sloApi = {
  list: async (filter?: SloServiceFilter) => {
    const { data } = await http.get<SloStatus[]>('/apm/slos', { params: filter ?? {} });
    return data;
  },
  get: async (id: string) => {
    const { data } = await http.get<SloDetail>(`/apm/slos/${encodeURIComponent(id)}`);
    return data;
  },
  create: async (write: SloWrite) => {
    const { data } = await http.post<ServiceLevelObjective>('/apm/slos', write);
    return data;
  },
  update: async (id: string, write: SloWrite) => {
    const { data } = await http.put<ServiceLevelObjective>(`/apm/slos/${encodeURIComponent(id)}`, write);
    return data;
  },
  remove: async (id: string) => {
    await http.delete(`/apm/slos/${encodeURIComponent(id)}`);
  },
};

export const sloQueryKeys = {
  all: ['apm', 'slos'] as const,
  list: (orgId: string, filter?: SloServiceFilter) => [...sloQueryKeys.all, 'list', orgId, filter ?? null] as const,
  detail: (orgId: string, id: string) => [...sloQueryKeys.all, 'detail', orgId, id] as const,
};
