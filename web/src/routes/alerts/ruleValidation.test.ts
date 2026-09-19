import { describe, expect, it } from 'vitest';

import {
  alertRuleActivationBlocker,
  alertRulePreviewFingerprint,
} from './alertRuleModel';

describe('alert rule validation', () => {
  it('normalizes whitespace when identifying validated query inputs', () => {
    expect(
      alertRulePreviewFingerprint({
        signal: 'metrics',
        queryLanguage: 'promql',
        streamName: ' process_cpu_usage ',
        statement: ' avg(process_cpu_usage) ',
      }),
    ).toBe(
      alertRulePreviewFingerprint({
        signal: 'metrics',
        queryLanguage: 'promql',
        streamName: 'process_cpu_usage',
        statement: 'avg(process_cpu_usage)',
      }),
    );
  });

  it('blocks activation while the query pre-check is running', () => {
    expect(
      alertRuleActivationBlocker({
        identityReady: true,
        previewRunning: true,
        queryReady: false,
        thresholdsReady: true,
        runbookReady: true,
      }),
    ).toBe('preview_running');
  });

  it('allows activation only after every visible check is ready', () => {
    expect(
      alertRuleActivationBlocker({
        identityReady: true,
        previewRunning: false,
        queryReady: true,
        thresholdsReady: true,
        runbookReady: true,
      }),
    ).toBeNull();
  });
});
