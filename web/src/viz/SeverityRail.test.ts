import { describe, expect, it } from 'vitest';

import { severityBarClass } from './SeverityRail';

describe('severityBarClass', () => {
  it('keeps alert, log, and stream categories on one color grammar', () => {
    expect(severityBarClass('ERROR')).toBe('bg-red');
    expect(severityBarClass('WARN')).toBe('bg-yellow');
    expect(severityBarClass('healthy')).toBe('bg-green');
    expect(severityBarClass('delayed')).toBe('bg-yellow');
    expect(severityBarClass('DEBUG')).toBe('bg-tx-3');
    expect(severityBarClass(undefined)).toBe('bg-blue');
  });
});
