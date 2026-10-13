import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as searchJobsApi from '@/api/searchJobs';
import type { SearchJob, SearchJobResultPage } from '@/api/searchJobs';

import { downloadJobResults } from './download';
import { RESULT_PAGE_SIZE } from './model';

vi.mock('@/api/searchJobs', async (importOriginal) => ({
  ...(await importOriginal<typeof searchJobsApi>()),
  results: vi.fn(),
}));

const api = vi.mocked(searchJobsApi);
const JOB = { job_id: 'job-1', state: 'done' } as SearchJob;

function page(rows: unknown[], hasMore: boolean): SearchJobResultPage {
  return {
    state: 'done',
    result_rows: null,
    page: 1,
    page_size: RESULT_PAGE_SIZE,
    rows,
    has_more: hasMore,
    error: null,
  };
}

/** What the browser was handed to save: the file's name and what is in it. */
let saved: Array<{ name: string; blob: Blob }> = [];

// jsdom's Blob has no `text()`.
const textOf = (blob: Blob) =>
  new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsText(blob);
  });

beforeEach(() => {
  saved = [];
  vi.clearAllMocks();
  let held: Blob | undefined;
  URL.createObjectURL = vi.fn((blob: Blob) => {
    held = blob;
    return 'blob:results';
  });
  URL.revokeObjectURL = vi.fn();
  vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function click(
    this: HTMLAnchorElement,
  ) {
    saved.push({ name: this.download, blob: held! });
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('downloadJobResults', () => {
  it('reads page after page until there is no more, and saves one line per row', async () => {
    api.results
      .mockResolvedValueOnce(page([{ a: 1 }, { a: 2 }], true))
      .mockResolvedValueOnce(page([{ a: 3 }], false));

    await expect(downloadJobResults(JOB)).resolves.toBe(3);

    expect(api.results.mock.calls).toEqual([
      ['job-1', 1, RESULT_PAGE_SIZE],
      ['job-1', 2, RESULT_PAGE_SIZE],
    ]);
    expect(saved).toHaveLength(1);
    expect(saved[0]!.name).toBe('search-job-job-1.ndjson');
    expect(saved[0]!.blob.type).toBe('application/x-ndjson');
    await expect(textOf(saved[0]!.blob)).resolves.toBe('{"a":1}\n{"a":2}\n{"a":3}\n');
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:results');
    // The link that carried the file is not left in the page.
    expect(document.querySelector('a[download]')).toBeNull();
  });

  it('stops at an empty page even when more is promised', async () => {
    api.results
      .mockResolvedValueOnce(page([{ a: 1 }], true))
      .mockResolvedValue(page([], true));

    await expect(downloadJobResults(JOB)).resolves.toBe(1);
    expect(api.results).toHaveBeenCalledTimes(2);
  });

  it('saves an empty file for a job that found nothing', async () => {
    api.results.mockResolvedValue(page([], false));

    await expect(downloadJobResults(JOB)).resolves.toBe(0);
    await expect(textOf(saved[0]!.blob)).resolves.toBe('');
  });

  it('saves nothing when a page cannot be read', async () => {
    api.results
      .mockResolvedValueOnce(page([{ a: 1 }], true))
      .mockRejectedValue(new Error('result object is gone'));

    await expect(downloadJobResults(JOB)).rejects.toThrow('result object is gone');
    expect(saved).toEqual([]);
  });
});
