import { describe, expect, it, vi } from 'vitest';

import type {
  ActiveMonitorRevision,
  CreateMonitorInput,
  MonitorDetail,
  MonitorRevision,
} from '@/api/synthetics';

import { saveCheckDraft } from './save';

const input = {} as CreateMonitorInput;

describe('saveCheckDraft', () => {
  it('creates a draft without trying to publish it', async () => {
    const createMonitor = vi.fn().mockResolvedValue({
      monitor: { id: 'monitor-new' },
    } as ActiveMonitorRevision);
    const createRevision = vi.fn();

    await expect(
      saveCheckDraft({ createMonitor, createRevision }, 'create', undefined, input),
    ).resolves.toBe('monitor-new');
    expect(createMonitor).toHaveBeenCalledWith(input);
    expect(createRevision).not.toHaveBeenCalled();
  });

  it('saves edits as a new draft revision', async () => {
    const createMonitor = vi.fn();
    const createRevision = vi.fn().mockResolvedValue({} as MonitorRevision);
    const detail = { monitor: { id: 'monitor-existing' } } as MonitorDetail;

    await expect(
      saveCheckDraft({ createMonitor, createRevision }, 'edit', detail, input),
    ).resolves.toBe('monitor-existing');
    expect(createRevision).toHaveBeenCalledWith('monitor-existing', input);
    expect(createMonitor).not.toHaveBeenCalled();
  });
});
