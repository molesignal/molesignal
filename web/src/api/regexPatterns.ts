import { http } from '@/lib/http';

export interface RegexPattern {
  id: string;
  orgId: string;
  name: string;
  pattern: string;
  description: string;
  /** Matched text is replaced with this (supports `$1` capture groups). */
  replacement: string;
  /** Permanently redact matches at write time; off = query-side `mask(col)` only. */
  applyOnIntake: boolean;
  createdAt: number;
  updatedAt: number;
}

interface RegexPatternWire extends Omit<RegexPattern, 'createdAt' | 'updatedAt'> {
  createdAt?: number;
  updatedAt?: number;
}

export interface CreateRegexPatternInput {
  name: string;
  pattern: string;
  description?: string;
  replacement?: string;
  applyOnIntake?: boolean;
}

export async function list(): Promise<RegexPattern[]> {
  const { data } = await http.get<RegexPatternWire[]>('/regex_patterns');
  return data.map(normalize);
}

export async function create(input: CreateRegexPatternInput): Promise<RegexPattern> {
  const { data } = await http.post<RegexPatternWire>('/regex_patterns', input);
  return normalize(data);
}

export async function update(id: string, input: CreateRegexPatternInput): Promise<RegexPattern> {
  const { data } = await http.put<RegexPatternWire>(
    `/regex_patterns/${encodeURIComponent(id)}`,
    input,
  );
  return normalize(data);
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/regex_patterns/${encodeURIComponent(id)}`);
}

function normalize(row: RegexPatternWire): RegexPattern {
  return {
    ...row,
    createdAt: row.createdAt ?? 0,
    updatedAt: row.updatedAt ?? 0,
  };
}
