import { http } from '@/lib/http';

/** Whose knowledge: the organization has one, every team one of its own. */
export interface KnowledgeScope {
  scope: 'org' | 'team';
  team_id?: string | null;
}

export interface KnowledgeBase extends KnowledgeScope {
  team_name?: string | null;
  files: number;
  bytes: number;
  /** MOLE.md is there: every chat of the scope reads it. */
  entry: boolean;
  entry_updated_at_micros?: number | null;
  /** Other files changed after MOLE.md was last written: it may be out of date. */
  changed_since_entry: number;
  updated_at_micros?: number | null;
  can_edit: boolean;
}

export interface KnowledgeLimits {
  max_file_bytes: number;
  max_total_bytes: number;
  max_files: number;
}

export interface KnowledgeOverview {
  bases: KnowledgeBase[];
  entry_file: string;
  limits: KnowledgeLimits;
}

export interface KnowledgeFileMeta {
  path: string;
  size_bytes: number;
  updated_by: string;
  updated_at_micros: number;
}

export interface KnowledgeFile extends KnowledgeFileMeta {
  content: string;
}

export const ENTRY_FILE = 'MOLE.md';

function params(scope: KnowledgeScope, path?: string): Record<string, string> {
  const result: Record<string, string> = { scope: scope.scope };
  if (scope.scope === 'team' && scope.team_id) result.team_id = scope.team_id;
  if (path !== undefined) result.path = path;
  return result;
}

export function scopeKey(scope: KnowledgeScope): string {
  return scope.scope === 'team' ? `team:${scope.team_id ?? ''}` : 'org';
}

export async function knowledgeOverview(): Promise<KnowledgeOverview> {
  const { data } = await http.get<KnowledgeOverview>('/agent/knowledge');
  return data;
}

export async function listKnowledgeFiles(
  scope: KnowledgeScope,
): Promise<{ files: KnowledgeFileMeta[]; can_edit: boolean }> {
  const { data } = await http.get<{ files: KnowledgeFileMeta[]; can_edit: boolean }>(
    '/agent/knowledge/files',
    { params: params(scope) },
  );
  return { files: data.files ?? [], can_edit: data.can_edit };
}

export async function readKnowledgeFile(scope: KnowledgeScope, path: string): Promise<KnowledgeFile> {
  const { data } = await http.get<KnowledgeFile>('/agent/knowledge/file', {
    params: params(scope, path),
  });
  return data;
}

export async function writeKnowledgeFile(
  scope: KnowledgeScope,
  path: string,
  content: string,
): Promise<void> {
  await http.put('/agent/knowledge/file', { ...params(scope), path, content });
}

/** Delete a file, or a folder with everything in it. */
export async function deleteKnowledgePath(scope: KnowledgeScope, path: string): Promise<void> {
  await http.delete('/agent/knowledge/file', { params: params(scope, path) });
}

export async function moveKnowledgeFile(
  scope: KnowledgeScope,
  from: string,
  to: string,
): Promise<void> {
  await http.post('/agent/knowledge/move', { ...params(scope), from, to });
}

export interface UploadOutcome {
  written: number;
  skipped: { path: string; reason: 'hidden_or_invalid_path' | 'too_large' | 'not_text' }[];
}

/** Upload text files, or a whole folder: each file's name is its path in the folder. */
export async function uploadKnowledge(
  scope: KnowledgeScope,
  files: readonly { path: string; file: File }[],
  dir = '',
): Promise<UploadOutcome> {
  const form = new FormData();
  form.append('scope', scope.scope);
  if (scope.scope === 'team' && scope.team_id) form.append('team_id', scope.team_id);
  if (dir) form.append('dir', dir);
  for (const { path, file } of files) form.append('file', file, path);
  const { data } = await http.post<UploadOutcome>('/agent/knowledge/upload', form);
  return data;
}
