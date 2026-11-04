import { http } from '@/lib/http';

export interface ReportTemplate {
  id: string;
  name: string;
  description: string;
  targetType: 'dashboard' | 'saved_view' | string;
  format: string;
  timeRangePreset: string;
  isBuiltin: boolean;
  createdAt?: number | null;
  updatedAt?: number | null;
}

export interface ReportTemplateInput {
  name: string;
  description: string;
  targetType: 'dashboard' | 'saved_view';
  format: string;
  timeRangePreset: string;
}

export async function list(): Promise<ReportTemplate[]> {
  const { data } = await http.get<ReportTemplate[]>('/report_templates');
  return data;
}

export async function create(input: ReportTemplateInput): Promise<ReportTemplate> {
  const { data } = await http.post<ReportTemplate>('/report_templates', input);
  return data;
}

export async function update(
  id: string,
  input: ReportTemplateInput,
): Promise<ReportTemplate> {
  const { data } = await http.put<ReportTemplate>(
    `/report_templates/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/report_templates/${encodeURIComponent(id)}`);
}
