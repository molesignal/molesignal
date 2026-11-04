import { http } from '@/lib/http';

export interface Folder {
  id: string;
  orgId: string;
  name: string;
  parentId?: string;
}

export interface FolderInput {
  name: string;
  parentId?: string;
}

type FolderListResponse = Folder[] | { items?: unknown[] };

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object';
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined;
}

function normalizeFolder(raw: unknown): Folder {
  const record = isRecord(raw) ? raw : {};
  const folder: Folder = {
    id: stringValue(record.id) ?? stringValue(record.uid) ?? stringValue(record.name) ?? '',
    orgId: stringValue(record.orgId) ?? '',
    name: stringValue(record.name) ?? 'Untitled folder',
  };
  const parentId = stringValue(record.parentId);
  if (parentId) folder.parentId = parentId;
  return folder;
}

function cleanInput(input: FolderInput): FolderInput {
  const payload: FolderInput = { name: input.name.trim() };
  const parentId = input.parentId?.trim();
  if (parentId) payload.parentId = parentId;
  return payload;
}

export async function list(): Promise<Folder[]> {
  const { data } = await http.get<FolderListResponse>('/folders');
  const items = Array.isArray(data) ? data : isRecord(data) && Array.isArray(data.items) ? data.items : [];
  return items.map(normalizeFolder).filter((folder) => folder.id);
}

export async function create(input: FolderInput): Promise<Folder> {
  const { data } = await http.post<Folder>('/folders', cleanInput(input));
  return normalizeFolder(data);
}

export async function update(id: string, input: FolderInput): Promise<Folder> {
  const { data } = await http.put<Folder>(`/folders/${encodeURIComponent(id)}`, cleanInput(input));
  return normalizeFolder(data);
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/folders/${encodeURIComponent(id)}`);
}
