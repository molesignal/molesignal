import { http } from '@/lib/http';

/** Build / version info echoed by the backend `/version` endpoint. */
export interface VersionInfo {
  version: string;
  commit: string;
  branch: string;
  buildEpochSecs: number;
  buildId: string;
  releaseChannel: string;
  edition: string;
}

export async function get(): Promise<VersionInfo> {
  const { data } = await http.get<VersionInfo>('/version');
  return data;
}
