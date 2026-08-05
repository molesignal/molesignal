import { http } from '@/lib/http';
import type { CursorPage } from '@/pagination/cursor';

export type MetricType = 'counter' | 'histogram' | 'gauge';

export interface MetricCatalogEntry {
  name: string;
  /** Optional so the web UI remains compatible with pre-type catalog responses. */
  metric_type?: MetricType;
  labels: string[];
  field_count: number;
}

export type MetricCatalogResponse = CursorPage<MetricCatalogEntry>;

export async function fetchMetricCatalog(params: {
  q?: string;
  limit?: number;
  cursor?: string;
} = {}): Promise<MetricCatalogResponse> {
  const { data } = await http.get<MetricCatalogResponse>('/metrics/catalog', { params });
  return data;
}
