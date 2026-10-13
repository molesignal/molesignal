import { describe, expect, it } from 'vitest';

import type { ScheduledReport } from '@/api/reports';

import {
  adaptReport,
  draftFromSeed,
  FORMAT_OPTIONS,
  normalizeFormat,
  reportInputFromDisplay,
  reportInputFromDraft,
  templateDraftFromPreset,
  templateFormatOptions,
  type TemplatePreset,
} from './reportTypes';

function stored(format: string): ScheduledReport {
  return {
    id: 'r1',
    name: 'Weekly SLO',
    dashboard_id: 'd1',
    saved_view_id: null,
    cron: 'every:7d',
    recipients: [{ kind: 'webhook', target: 'https://example.com/hook' }],
    format,
    enabled: true,
  };
}

describe('report formats', () => {
  it('offers every format the backend generates', () => {
    expect(FORMAT_OPTIONS.map((option) => option.value)).toEqual([
      'pdf',
      'png',
      'csv',
      'json',
      'svg',
    ]);
  });

  it.each(['pdf', 'png', 'csv', 'json', 'svg'] as const)(
    'keeps a %s report in its format when it is enabled, paused or saved again',
    (format) => {
      const report = adaptReport(stored(format));

      expect(report.format).toBe(format);
      // The switch in the list saves the report as the list shows it.
      expect(reportInputFromDisplay(report, false).format).toBe(format);
      // The workbench saves the draft it was opened with.
      const draft = draftFromSeed({ report, template: null });
      expect(reportInputFromDraft(draft).format).toBe(format);
    },
  );

  it('reads a stored format whatever its case, and anything else as PDF', () => {
    expect(normalizeFormat('SVG')).toBe('svg');
    expect(normalizeFormat('Csv')).toBe('csv');
    expect(normalizeFormat('')).toBe('pdf');
    expect(normalizeFormat('xlsx')).toBe('pdf');
  });
});

describe('template formats', () => {
  it('offers what the template API takes', () => {
    expect(templateFormatOptions('dashboard').map((option) => option.value)).toEqual([
      'pdf',
    ]);
    expect(templateFormatOptions('saved_view').map((option) => option.value)).toEqual([
      'pdf',
      'csv',
      'json',
    ]);
  });

  it('opens a template in a format the template API takes', () => {
    const preset: TemplatePreset = {
      id: 'remote:t1',
      serverId: 't1',
      isBuiltin: false,
      name: 'Query snapshot',
      description: '',
      sourceKind: 'saved_view',
      format: 'svg',
      rangePreset: 'previous-24-hours',
      icon: 'query',
    };

    expect(templateDraftFromPreset(preset).format).toBe('pdf');
    expect(templateDraftFromPreset({ ...preset, format: 'png' }).format).toBe('pdf');
    expect(templateDraftFromPreset({ ...preset, format: 'csv' }).format).toBe('csv');
  });
});
