import { describe, expect, it } from 'vitest';

import type { StreamSummary } from '@/api/streams';

import {
  buildLogServiceErrorQuery,
  chooseServiceForErrorInvestigation,
  logObservationsFromResult,
} from './starterService';

function logStream(fields: string[], name = 'app_logs'): StreamSummary {
  return {
    id: `logs:${name}`,
    label: name,
    name,
    streamType: 'logs',
    type: 'logs',
    schema: {
      fields: fields.map((field) => ({
        name: field,
        dataType: 'utf8',
        nullable: true,
        indexed: false,
      })),
    },
    retention: null,
    effectiveRetention: { days: 30 },
    settings: {
      description: null,
      indexRules: [],
      maxQueryRangeHours: null,
      queryable: true,
    },
    createdAt: 0,
    updatedAt: 0,
  };
}

describe('starter service discovery', () => {
  it('builds an aggregate query from the real stream schema', () => {
    const statement = buildLogServiceErrorQuery(
      logStream(['service.name', 'severity_text'], 'logs"prod'),
    );

    expect(statement).toContain('"service.name" AS service');
    expect(statement).toContain('LOWER(CAST("severity_text" AS VARCHAR))');
    expect(statement).toContain('FROM "logs""prod"');
    expect(statement).not.toContain('checkout-api');
  });

  it('skips streams without service and severity fields', () => {
    expect(buildLogServiceErrorQuery(logStream(['message']))).toBeNull();
  });

  it('parses aggregated log error rates', () => {
    expect(logObservationsFromResult({
      columns: ['service', 'total_count', 'error_count'],
      rows: [
        ['catalog-api', 200, 20],
        ['checkout-api', '100', '5'],
      ],
      scannedRows: 300,
      tookMs: 4,
    })).toEqual([
      {
        service: 'catalog-api',
        source: 'logs',
        errorRate: 0.1,
        sampleCount: 200,
      },
      {
        service: 'checkout-api',
        source: 'logs',
        errorRate: 0.05,
        sampleCount: 100,
      },
    ]);
  });

  it('combines log and trace error rates when selecting a service', () => {
    expect(chooseServiceForErrorInvestigation([
      {
        service: 'catalog-api',
        source: 'logs',
        errorRate: 0.08,
        sampleCount: 500,
      },
      {
        service: 'catalog-api',
        source: 'traces',
        errorRate: 0.06,
        sampleCount: 400,
      },
      {
        service: 'checkout-api',
        source: 'logs',
        errorRate: 0.1,
        sampleCount: 500,
      },
      {
        service: 'checkout-api',
        source: 'traces',
        errorRate: 0,
        sampleCount: 400,
      },
    ])).toBe('catalog-api');
  });

  it('does not claim an increase when no real errors were observed', () => {
    expect(chooseServiceForErrorInvestigation([
      {
        service: 'healthy-api',
        source: 'traces',
        errorRate: 0,
        sampleCount: 5_000,
      },
    ])).toBeNull();
  });
});
