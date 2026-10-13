import * as searchJobsApi from '@/api/searchJobs';

import { RESULT_PAGE_SIZE, resultFileName } from './model';

/**
 * Reads every row of a finished job, a page at a time, and hands them to the
 * browser as one NDJSON file: the form the rows are kept in. Returns how many
 * rows the file holds.
 */
export async function downloadJobResults(job: searchJobsApi.SearchJob): Promise<number> {
  const lines: string[] = [];

  for (let page = 1; ; page += 1) {
    const result = await searchJobsApi.results(job.job_id, page, RESULT_PAGE_SIZE);
    for (const row of result.rows) lines.push(JSON.stringify(row));
    // An empty page ends the walk even if the count on the job says there is more.
    if (!result.has_more || result.rows.length === 0) break;
  }

  saveTextFile(
    resultFileName(job),
    lines.length > 0 ? `${lines.join('\n')}\n` : '',
    'application/x-ndjson',
  );
  return lines.length;
}

function saveTextFile(filename: string, value: string, type: string): void {
  const url = URL.createObjectURL(new Blob([value], { type }));
  const link = document.createElement('a');
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}
