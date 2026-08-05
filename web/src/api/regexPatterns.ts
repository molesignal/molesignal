import { http } from '@/lib/http';

export interface RegexPattern {
  id: string;
  org_id: string;
  name: string;
  pattern: string;
  description: string;
  /** Matched text is replaced with this (supports `$1` capture groups). */
  replacement: string;
  /** Permanently redact matches at write time; off = query-side `mask(col)` only. */
  apply_on_ingest: boolean;
  created_at_micros: number;
  updated_at_micros: number;
}

interface RegexPatternWire extends Omit<RegexPattern, 'created_at_micros' | 'updated_at_micros'> {
  created_at?: number;
  updated_at?: number;
  created_at_micros?: number;
  updated_at_micros?: number;
}

export interface CreateRegexPatternInput {
  name: string;
  pattern: string;
  description?: string;
  replacement?: string;
  apply_on_ingest?: boolean;
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
    created_at_micros: row.created_at_micros ?? row.created_at ?? 0,
    updated_at_micros: row.updated_at_micros ?? row.updated_at ?? 0,
  };
}
