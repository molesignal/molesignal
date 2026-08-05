import { AxiosError } from 'axios';

import { http } from '@/lib/http';

export interface ScheduledReport {
  id: string;
  org_id?: string;
  name: string;
  description?: string;
  dashboard_id?: string | null;
  saved_view_id?: string | null;
  cron?: string;
  enabled?: boolean;
  recipients?: ReportRecipient[];
  format?: string;
  time_range_json?: unknown;
  last_run_at_micros?: number | null;
  created_at_micros?: number;
  updated_at_micros?: number;
  // Backend may include other fields; we keep the shape permissive so the
  // table can show whatever's there without crashing on extra keys.
  [key: string]: unknown;
}

export interface ReportRecipient {
  kind: string;
  target: string;
}

export interface ReportInput {
  name: string;
  dashboard_id: string | null;
  saved_view_id: string | null;
  cron: string;
  recipients: ReportRecipient[];
  format: string;
  time_range_json: unknown;
  enabled: boolean;
}

export interface ReportDelivery {
  id: string;
  report_id?: string;
  org_id?: string;
  status?: 'pending' | 'sent' | 'failed' | string;
  attempt?: number;
  recipient_kind?: string;
  recipient_target?: string;
  error?: string | null;
  attempted_at?: number;
  [key: string]: unknown;
}

export async function list(): Promise<ScheduledReport[]> {
  const { data } = await http.get<ScheduledReport[] | { items: ScheduledReport[] }>(
    '/scheduled_reports',
  );
  return Array.isArray(data) ? data : data.items;
}

export async function deliveries(id: string): Promise<ReportDelivery[]> {
  const { data } = await http.get<ReportDelivery[] | { items: ReportDelivery[] }>(
    `/scheduled_reports/${encodeURIComponent(id)}/deliveries`,
  );
  return Array.isArray(data) ? data : data.items;
}

export async function create(input: ReportInput): Promise<ScheduledReport> {
  const { data } = await http.post<ScheduledReport>('/scheduled_reports', input);
  return data;
}

export async function update(id: string, input: ReportInput): Promise<ScheduledReport> {
  const { data } = await http.put<ScheduledReport>(
    `/scheduled_reports/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/scheduled_reports/${encodeURIComponent(id)}`);
}

export async function preview(id: string): Promise<Blob> {
  try {
    const { data } = await http.get<Blob>(
      `/scheduled_reports/${encodeURIComponent(id)}/preview`,
      { responseType: 'blob' },
    );
    return data;
  } catch (error) {
    // responseType=blob 也会把后端 JSON 错误包装成 Blob；解析回来后交给全局
    // toApiError，确保用户看到 renderer 不可用等真实原因。
    if (error instanceof AxiosError && error.response?.data instanceof Blob) {
      const blob = error.response.data;
      if (blob.type.includes('application/json')) {
        try {
          error.response.data = JSON.parse(await blob.text());
        } catch {
          // 保留原始 AxiosError。
        }
      }
    }
    throw error;
  }
}

export async function downloadPreview(id: string, filename: string): Promise<void> {
  const blob = await preview(id);
  await assertDownloadSignature(blob, filename);
  const url = URL.createObjectURL(blob);
  try {
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = filename;
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
  } finally {
    URL.revokeObjectURL(url);
  }
}

async function assertDownloadSignature(blob: Blob, filename: string): Promise<void> {
  const normalized = filename.toLocaleLowerCase();
  if (normalized.endsWith('.pdf')) {
    const header = await blob.slice(0, 5).text();
    if (header !== '%PDF-') {
      throw new Error('Report export returned an invalid PDF payload');
    }
  }
  if (normalized.endsWith('.png')) {
    const header = new Uint8Array(await blob.slice(0, 8).arrayBuffer());
    const signature = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    if (!signature.every((value, index) => header[index] === value)) {
      throw new Error('Report export returned an invalid PNG payload');
    }
  }
}
