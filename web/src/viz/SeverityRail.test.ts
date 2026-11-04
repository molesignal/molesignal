import { describe, expect, it } from 'vitest';

import { severityBarClass } from './SeverityRail';

describe('severityBarClass', () => {
  it('keeps alert, log, and stream categories on one color grammar, in the tones\' fills', () => {
    expect(severityBarClass('ERROR')).toBe('bg-[var(--red-fill)]');
    expect(severityBarClass('WARN')).toBe('bg-[var(--yellow-fill)]');
    expect(severityBarClass('healthy')).toBe('bg-[var(--green-fill)]');
    expect(severityBarClass('delayed')).toBe('bg-[var(--yellow-fill)]');
    expect(severityBarClass('DEBUG')).toBe('bg-tx-3');
    expect(severityBarClass(undefined)).toBe('bg-[var(--blue-fill)]');
  });
});
