import { describe, expect, it } from 'vitest';

import { COLLECTOR_GROUPS, configFromDraft, draftFromConfig } from './config';

describe('node configuration form', () => {
  it('starts with every collector running and writes nothing for an empty configuration', () => {
    const draft = draftFromConfig({}, []);
    expect(draft.enabled).toHaveLength(Object.values(COLLECTOR_GROUPS).flat().length);
    expect(configFromDraft(draft)).toEqual({ config: {} });
  });

  it('writes unchecked collectors as deny and keeps only the values set', () => {
    const draft = draftFromConfig({}, ['custom_probe']);
    draft.enabled = draft.enabled.filter((name) => name !== 'oom' && name !== 'custom_probe');
    draft.oncpu = true;
    draft.frequencyHz = '49';
    draft.reclaimThresholdMs = '500';

    expect(configFromDraft(draft)).toEqual({
      config: {
        collectors: { deny: ['oom', 'custom_probe'] },
        profiling: { oncpu: true, frequency_hz: 49 },
        memory_reclaim_blocked_threshold_ns: 500_000_000,
      },
    });
  });

  it('reads allow and deny back into the checklist', () => {
    const draft = draftFromConfig(
      { collectors: { allow: ['loadavg', 'oom'], deny: ['oom'] }, profiling: { interval_secs: 20 } },
      [],
    );

    expect(draft.enabled).toEqual(['loadavg']);
    expect(draft.intervalSecs).toBe('20');
  });

  it('names the first number out of range', () => {
    const draft = draftFromConfig({}, []);
    draft.frequencyHz = '5000';
    expect(configFromDraft(draft)).toEqual({ field: 'frequencyHz' });
  });
});
