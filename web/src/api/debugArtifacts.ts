import { http } from '@/lib/http';

export type DebugArtifactKind =
  | 'javascript_sourcemap'
  | 'flutter_symbols'
  | 'android_mapping'
  | 'android_native_symbols'
  | 'apple_dsym';

export interface DebugArtifactMeta {
  id: string;
  application_id: string;
  service: string;
  release: string;
  kind: DebugArtifactKind;
  platform: string;
  architecture: string;
  debug_id: string;
  filename: string;
  size_bytes: number;
  checksum_sha256: string;
  uploaded_at_micros: number;
}

export interface ListParams {
  application_id?: string;
  service?: string;
  kind?: DebugArtifactKind;
  platform?: string;
}

export async function list(params: ListParams = {}): Promise<DebugArtifactMeta[]> {
  const { data } = await http.get<DebugArtifactMeta[]>('/debug-artifacts', { params });
  return data;
}

export interface UploadParams {
  application_id: string;
  service: string;
  release: string;
  kind: DebugArtifactKind;
  platform: string;
  architecture?: string;
  debug_id?: string;
  file: File;
}

export async function upload(params: UploadParams): Promise<DebugArtifactMeta> {
  const form = new FormData();
  form.append('application_id', params.application_id);
  form.append('service', params.service);
  form.append('release', params.release);
  form.append('kind', params.kind);
  form.append('platform', params.platform);
  if (params.architecture) form.append('architecture', params.architecture);
  if (params.debug_id) form.append('debug_id', params.debug_id);
  form.append('file', params.file, params.file.name);
  const { data } = await http.post<DebugArtifactMeta>('/debug-artifacts', form);
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/debug-artifacts/${encodeURIComponent(id)}`);
}
