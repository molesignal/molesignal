import { describe, expect, it } from 'vitest';

import type { AuditRetentionPreview, StreamRetentionPreview } from '@/api/instance';

import {
  auditSaveStep,
  isShortening,
  LARGE_AUDIT_IMPACT_ROWS,
  LARGE_STREAM_IMPACT,
  type RetentionPreviewState,
  streamSaveStep,
} from './model';

const STREAMS: StreamRetentionPreview = {
  retentionDays: 7,
  previousRetentionDays: 30,
  estimated: true,
  generatedAt: 1,
  rows: 10,
  storedBytes: 100,
  earliestAt: 1,
  streamCount: 1,
  streams: [],
  unavailableStreams: 0,
};

const AUDIT: AuditRetentionPreview = {
  retentionDays: 30,
  previousRetentionDays: 90,
  rows: 10,
  earliestAt: 1,
};

const ready = <T,>(preview: T): RetentionPreviewState<T> => ({ status: 'ready', preview });

describe('isShortening', () => {
  it('is fewer days than stored', () => {
    expect(isShortening(30, 7)).toBe(true);
    expect(isShortening(30, 30)).toBe(false);
    expect(isShortening(30, 90)).toBe(false);
  });

  it('takes 0 as keeping for good, the longest there is', () => {
    expect(isShortening(0, 90)).toBe(true);
    expect(isShortening(90, 0)).toBe(false);
    expect(isShortening(0, 0)).toBe(false);
  });
});

describe('what Save does for streams', () => {
  it('just saves when the change deletes nothing more', () => {
    expect(streamSaveStep({ status: 'idle' })).toBe('save');
    expect(streamSaveStep(ready({ ...STREAMS, rows: 0, storedBytes: 0, streamCount: 0 }))).toBe('save');
  });

  it('asks for a click under the line and for the days typed from it on', () => {
    expect(streamSaveStep(ready(STREAMS))).toBe('confirm');

    expect(
      streamSaveStep(ready({ ...STREAMS, rows: LARGE_STREAM_IMPACT.rows - 1, storedBytes: LARGE_STREAM_IMPACT.storedBytes - 1 })),
    ).toBe('confirm');

    expect(streamSaveStep(ready({ ...STREAMS, rows: LARGE_STREAM_IMPACT.rows }))).toBe('type');
    expect(streamSaveStep(ready({ ...STREAMS, storedBytes: LARGE_STREAM_IMPACT.storedBytes }))).toBe('type');
  });

  it('treats what it could not tell as a lot', () => {
    expect(streamSaveStep({ status: 'pending' })).toBe('type');
    expect(streamSaveStep({ status: 'failed', retry: () => {} })).toBe('type');

    // Nothing found in the streams it could read says nothing about the two it could not.
    expect(
      streamSaveStep(ready({ ...STREAMS, rows: 0, storedBytes: 0, streamCount: 0, unavailableStreams: 2 })),
    ).toBe('type');
  });
});

describe('what Save does for audit records', () => {
  it('just saves when no record goes', () => {
    expect(auditSaveStep({ status: 'idle' })).toBe('save');
    expect(auditSaveStep(ready({ ...AUDIT, rows: 0, earliestAt: null }))).toBe('save');
  });

  it('asks for the days typed from the line on', () => {
    expect(auditSaveStep(ready({ ...AUDIT, rows: LARGE_AUDIT_IMPACT_ROWS - 1 }))).toBe('confirm');
    expect(auditSaveStep(ready({ ...AUDIT, rows: LARGE_AUDIT_IMPACT_ROWS }))).toBe('type');
  });

  it('treats records it could not count as a lot', () => {
    expect(auditSaveStep({ status: 'failed', retry: () => {} })).toBe('type');
  });
});
