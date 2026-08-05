import { http } from '@/lib/http';

/** Build / version info echoed by the backend `/version` endpoint. */
export interface VersionInfo {
  version: string;
  commit: string;
  branch: string;
  build_epoch_secs: number;
  build_id: string;
  release_channel: string;
  edition: string;
}

export async function get(): Promise<VersionInfo> {
  const { data } = await http.get<VersionInfo>('/version');
  return data;
}
