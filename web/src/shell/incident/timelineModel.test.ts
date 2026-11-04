import { describe, expect, it } from 'vitest';

import {
  categoryOfEntry,
  incidentPhases,
  pipelineChangeText,
  pipelineTraceOf,
} from './timelineModel';

describe('pipeline traces', () => {
  it('say each change in a few words, under its field', () => {
    expect(pipelineChangeText({ field: 'severity', from: 'error', to: 'critical' })).toEqual({
      group: 'severity',
      text: 'error → critical',
    });
    expect(pipelineChangeText({ field: 'labels.team', to: 'payments' })).toEqual({
      group: 'labels',
      text: '+team=payments',
    });
    expect(pipelineChangeText({ field: 'labels.k8s.pod', from: 'a' })).toEqual({
      group: 'labels',
      text: '-k8s.pod=a',
    });
    expect(pipelineChangeText({ field: 'annotations.owner', from: 'a', to: 'b' })).toEqual({
      group: 'annotations',
      text: 'owner: a → b',
    });
  });

  it('read what a row holds, and nothing it does not', () => {
    expect(pipelineTraceOf({ pipeline: 'p', changes: [{ field: 'summary' }], more: 2, error: 'x' })).toEqual({
      pipeline: 'p',
      changes: [{ field: 'summary' }],
      more: 2,
      error: 'x',
    });
    expect(pipelineTraceOf(null)).toEqual({ pipeline: '', changes: [], more: 0, error: null });
  });
});

describe('categoryOfEntry', () => {
  it('sets what was written apart, whoever wrote it', () => {
    expect(categoryOfEntry({ kind: 'note', actor: 'u1' })).toBe('notes');
    expect(categoryOfEntry({ kind: 'status_update', actor: 'u1' })).toBe('notes');
  });

  it('tells a person’s step from the alert’s own', () => {
    expect(categoryOfEntry({ kind: 'acknowledged', actor: 'u1' })).toBe('people');
    expect(categoryOfEntry({ kind: 'escalated', actor: null })).toBe('alert');
    expect(categoryOfEntry({ kind: 'escalated', actor: 'u1' })).toBe('people');
  });
});

describe('incidentPhases', () => {
  it('measures each moment from when it fired', () => {
    expect(incidentPhases({ createdAt: 100, acknowledgedAt: 160, resolvedAt: 400 })).toEqual([
      { key: 'fired', after: 0 },
      { key: 'acknowledged', after: 60 },
      { key: 'resolved', after: 300 },
    ]);
  });

  it('leaves empty what has not happened', () => {
    expect(incidentPhases({ createdAt: 100 }).map((phase) => phase.after)).toEqual([0, null, null]);
  });
});
