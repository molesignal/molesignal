import { http } from '@/lib/http';

/** Whether the built-in sample telemetry has been loaded into this org. */
export interface SampleDataStatus {
  loaded: boolean;
}

export interface LoadedStream {
  stream: string;
  rows: number;
}

export interface LoadSampleResult {
  loaded: boolean;
  total_rows: number;
  streams: LoadedStream[];
}

/** `GET /onboarding/sample-data` — probe whether sample streams exist. */
export async function getSampleDataStatus(): Promise<SampleDataStatus> {
  const { data } = await http.get<SampleDataStatus>('/onboarding/sample-data');
  return data;
}

/** `POST /onboarding/sample-data` — ingest the built-in cross-signal demo
 *  (logs + metrics + traces sharing trace_ids) into the current org. */
export async function loadSampleData(): Promise<LoadSampleResult> {
  const { data } = await http.post<LoadSampleResult>('/onboarding/sample-data');
  return data;
}
