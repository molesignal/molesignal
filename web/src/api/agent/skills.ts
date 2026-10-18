import { http } from '@/lib/http';

/** Where a skill comes from. Built-in and instance-folder skills are read-only. */
export type SkillSource = 'builtin' | 'path' | 'upload' | 'repository';

/** Whose a skill is; `instance` for the built-in and instance-folder ones. */
export type SkillScope = 'instance' | 'org' | 'team' | 'personal';

export interface Skill {
  /** The key of an instance skill, or the id of a saved one. */
  id: string;
  name: string;
  /** The title in the reader's language for a built-in skill, otherwise the name. */
  title: string;
  description: string;
  version?: string | null;
  source: SkillSource;
  scope: SkillScope;
  team_id?: string | null;
  owner_id?: string | null;
  enabled: boolean;
  /** The one who reads can change and move it. */
  editable: boolean;
  /** The one who reads can turn it off and remove it. */
  moderatable: boolean;
  hash: string;
  file_count: number;
  argument_hint?: string | null;
  allowed_tools: string[];
  tags: string[];
  examples: string[];
  requires_runner: boolean;
  repository_id?: string | null;
  package_name?: string | null;
  /** The folder of an instance skill, or of a skill in a repository. */
  dir?: string | null;
  updated_at_micros?: number | null;
}

export interface SkillFile {
  path: string;
  size: number;
  /** Shown as text and readable by the model. */
  text: boolean;
}

export interface SkillDetail extends Skill {
  /** `SKILL.md` without its frontmatter. */
  body: string;
  files: SkillFile[];
}

/** A folder under the instance skill paths that could not be read. */
export interface SkillPathProblem {
  dir: string;
  message: string;
}

export interface SkillPaths {
  roots: string[];
  /** Only administrators see them. */
  problems: SkillPathProblem[];
  scanned_at_micros?: number | null;
}

export async function listSkills(): Promise<{ skills: Skill[]; paths: SkillPaths }> {
  const { data } = await http.get<{ skills: Skill[]; paths: SkillPaths }>('/agent/skills');
  return {
    skills: data.skills ?? [],
    paths: data.paths ?? { roots: [], problems: [] },
  };
}

const path = (id: string) => `/agent/skills/${encodeURIComponent(id)}`;

export async function getSkill(id: string): Promise<SkillDetail> {
  const { data } = await http.get<SkillDetail>(path(id));
  return data;
}

export async function getSkillFile(id: string, file: string): Promise<string> {
  const { data } = await http.get<{ path: string; content: string }>(`${path(id)}/file`, {
    params: { path: file },
  });
  return data.content;
}

/**
 * The package of a skill: what was uploaded, or a `.zip` made of its files. Read
 * through the API client, which carries the sign-in.
 */
export async function downloadSkillPackage(id: string): Promise<{ blob: Blob; fileName: string }> {
  const response = await http.get<Blob>(`${path(id)}/package`, { responseType: 'blob' });
  const disposition = String(response.headers['content-disposition'] ?? '');
  const fileName = /filename="([^"]+)"/.exec(disposition)?.[1] ?? `${id}.zip`;
  return { blob: response.data, fileName };
}

export interface SkillScopeInput {
  scope: 'org' | 'team' | 'personal';
  team_id?: string | undefined;
}

/**
 * Uploads a `.zip` or `.tar.gz` package. A skill of the same name in the scope
 * is replaced only with `replace`; otherwise the backend answers 409 with
 * `skill_exists`.
 */
export async function uploadSkill(
  file: File,
  target: SkillScopeInput,
  replace = false,
): Promise<void> {
  const form = new FormData();
  form.append('file', file);
  form.append('scope', target.scope);
  if (target.team_id) form.append('team_id', target.team_id);
  if (replace) form.append('replace', 'true');
  await http.post('/agent/skills', form);
}

/** Turns a skill on or off, or moves it to another scope. */
export async function updateSkill(
  id: string,
  change: { enabled?: boolean } & Partial<SkillScopeInput>,
): Promise<SkillDetail> {
  const { data } = await http.patch<SkillDetail>(path(id), change);
  return data;
}

export async function deleteSkill(id: string): Promise<void> {
  await http.delete(path(id));
}

/** Reads the instance skill paths again (administrators). */
export async function rescanSkillPaths(): Promise<SkillPaths> {
  const { data } = await http.post<SkillPaths>('/agent/skills/rescan');
  return data;
}

/** Where a repository is hosted; `archive` takes an address of its own. */
export type RepoHost = 'github' | 'gitlab' | 'gitea' | 'bitbucket' | 'archive';

export type RepositoryStatus = 'pending' | 'ok' | 'partial' | 'failed';

/** Whose a repository is, as the backend writes it. */
export type RepositoryScope =
  | { scope: 'org' }
  | { scope: 'team'; team_id: string }
  | { scope: 'personal'; owner_id: string };

export interface SkillRepository {
  id: string;
  org_id: string;
  scope: RepositoryScope;
  url: string;
  host: RepoHost;
  archive_url?: string | null;
  /** A branch or a tag; empty is the default branch. */
  git_ref: string;
  /** The folder of the repository the skills are in; empty is all of it. */
  dir: string;
  auto_sync: boolean;
  token_set: boolean;
  token_last4?: string | null;
  status: RepositoryStatus;
  last_error?: string | null;
  last_commit?: string | null;
  last_synced_at?: number | null;
  created_by: string;
  updated_by: string;
  editable: boolean;
}

export interface SyncReport {
  status: RepositoryStatus;
  imported: number;
  updated: number;
  unchanged: number;
  removed: number;
  /** Folders not imported and why, or why the whole sync failed. */
  problems: string[];
  commit?: string | null;
}

export interface RepositoryInput extends SkillScopeInput {
  url: string;
  /** Read from the address when left out. */
  host?: RepoHost | 'auto';
  archive_url?: string;
  git_ref: string;
  dir: string;
  auto_sync: boolean;
  /** Write-only; left out keeps the saved one. */
  token?: string;
  clear_token?: boolean;
}

export async function listSkillRepositories(): Promise<SkillRepository[]> {
  const { data } = await http.get<{ repositories: SkillRepository[] }>(
    '/agent/skill-repositories',
  );
  return data.repositories ?? [];
}

const repositoryPath = (id: string) => `/agent/skill-repositories/${encodeURIComponent(id)}`;

export async function createSkillRepository(
  input: RepositoryInput,
): Promise<{ repository: SkillRepository; sync: SyncReport }> {
  const { data } = await http.post<{ repository: SkillRepository; sync: SyncReport }>(
    '/agent/skill-repositories',
    input,
  );
  return data;
}

export async function updateSkillRepository(
  id: string,
  input: RepositoryInput,
): Promise<{ repository: SkillRepository; sync: SyncReport }> {
  const { data } = await http.put<{ repository: SkillRepository; sync: SyncReport }>(
    repositoryPath(id),
    input,
  );
  return data;
}

export async function deleteSkillRepository(id: string): Promise<void> {
  await http.delete(repositoryPath(id));
}

export async function syncSkillRepository(id: string): Promise<SyncReport> {
  const { data } = await http.post<{ sync: SyncReport }>(`${repositoryPath(id)}/sync`);
  return data.sync;
}
