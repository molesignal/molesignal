import { describe, expect, it } from 'vitest';

import { isReportRender, reportTimezone } from './mode';

describe('report render mode', () => {
  it('is asked for by report_render=1 and nothing else', () => {
    expect(isReportRender(new URLSearchParams('report_render=1'))).toBe(true);
    expect(isReportRender(new URLSearchParams('view=v1&report_render=1&time=now-7d..now'))).toBe(true);

    expect(isReportRender(new URLSearchParams(''))).toBe(false);
    expect(isReportRender(new URLSearchParams('report_render=0'))).toBe(false);
    expect(isReportRender(new URLSearchParams('report_render=true'))).toBe(false);
    expect(isReportRender(new URLSearchParams('render=1'))).toBe(false);
  });

  it('reads the timezone the report names', () => {
    expect(reportTimezone(new URLSearchParams('report_tz=Asia%2FShanghai'))).toBe('Asia/Shanghai');
    expect(reportTimezone(new URLSearchParams('report_tz=Etc%2FGMT%2B8'))).toBe('Etc/GMT+8');
    expect(reportTimezone(new URLSearchParams('report_tz=UTC'))).toBe('UTC');
  });

  it('leaves the reader their own timezone when the report names none it can use', () => {
    expect(reportTimezone(new URLSearchParams(''))).toBeNull();
    expect(reportTimezone(new URLSearchParams('report_tz='))).toBeNull();
    expect(reportTimezone(new URLSearchParams('report_tz=Mars%2FOlympus'))).toBeNull();
    // An unencoded `+` reads as a space, which is not a zone.
    expect(reportTimezone(new URLSearchParams('report_tz=Etc/GMT+8'))).toBeNull();
  });
});
