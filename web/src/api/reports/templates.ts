import { http } from '@/lib/http';

export interface ReportTemplate {
  id: string;
  name: string;
  description: string;
  target_type: 'dashboard' | 'saved_view' | string;
  format: string;
  time_range_preset: string;
  is_builtin: boolean;
  created_at_micros?: number | null;
  updated_at_micros?: number | null;
}

export interface ReportTemplateInput {
  name: string;
  description: string;
  target_type: 'dashboard' | 'saved_view';
  format: string;
  time_range_preset: string;
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
