import { http } from '@/lib/http';

/** Build / version info echoed by the backend `/version` endpoint. */
export interface VersionInfo {
  version: string;
  commit: string;
  branch: string;
  buildEpochSecs: number;
  buildId: string;
  edition: string;
}

export async function get(): Promise<VersionInfo> {
  const { data } = await http.get<VersionInfo>('/version');
  return data;
}

export interface UpdateInfo {
  currentVersion: string;
  latestVersion: string | null;
  releaseUrl: string | null;
  updateAvailable: boolean;
  checkedAt: number | null;
  status: 'ok' | 'unavailable';
}

export async function updates(refresh = false): Promise<UpdateInfo> {
  const { data } = await http.get<UpdateInfo>('/version/updates', { params: { refresh } });
  return data;
}
