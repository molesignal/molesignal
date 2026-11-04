import { describe, expect, it } from 'vitest';

import type { DashboardDraftPreview } from '@/api/agent/dashboardAuthoring';
import { createEmptyDashboardDefinition } from '@/dashboard-engine/model';

import {
  dashboardCreationIdempotencyKey,
  dashboardDraftAction,
  effectiveDraftExpiry,
  preflightRange,
  uniqueDraftWarnings,
} from './model';
import { dashboardStarterSelection } from './starter';

const NOW = 1_800_000_000_000_000;

function draft(
  patch: Partial<DashboardDraftPreview> = {},
): DashboardDraftPreview {
  return {
    draftId: 'draft-1',
    modelHash: 'a'.repeat(64),
    status: 'ready',
    createdAt: NOW - 60_000_000,
    expiresAt: NOW + 600_000_000,
    compiledModel: createEmptyDashboardDefinition('AI preview'),
    warnings: [],
    preflight: { panels: [], warnings: [], issues: [] },
    operation: null,
    dashboardId: null,
    dashboardRoute: null,
    canPropose: true,
    ...patch,
  };
}

describe('Dashboard authoring presentation model', () => {
  it('builds an explicit, complete Dashboard starter payload', () => {
    expect(dashboardStarterSelection('Build service metrics for the last hour')).toEqual({
      prompt: 'Build service metrics for the last hour',
      rangePreset: '1h',
      mode: 'auto',
      capability: 'dashboard_authoring',
      executionPolicy: 'policy',
    });
  });

  it('requires proposal, confirmation, and review in distinct states', () => {
    expect(dashboardDraftAction(draft(), NOW)).toBe('propose');
    expect(
      dashboardDraftAction(
        draft({
          operation: {
            approvalId: 'approval-1',
            status: 'approved',
            requiredApprovals: 0,
            approvedReviews: 0,
          },
        }),
        NOW,
      ),
    ).toBe('execute');
    expect(
      dashboardDraftAction(
        draft({
          operation: {
            approvalId: 'approval-2',
            status: 'pending',
            requiredApprovals: 2,
            approvedReviews: 1,
          },
        }),
        NOW,
      ),
    ).toBe('wait_for_review');
  });

  it('blocks on either draft or approval expiry and opens consumed results', () => {
    const approvalExpired = draft({
      operation: {
        approvalId: 'approval-1',
        status: 'approved',
        requiredApprovals: 0,
        approvedReviews: 0,
        expiresAt: NOW - 1,
      },
    });
    expect(effectiveDraftExpiry(approvalExpired)).toBe(NOW - 1);
    expect(dashboardDraftAction(approvalExpired, NOW)).toBe('blocked');
    expect(
      dashboardDraftAction(
        draft({
          status: 'consumed',
          dashboardId: 'dashboard-1',
          dashboardRoute: '/dashboards/dashboard-1',
          canPropose: false,
        }),
        NOW,
      ),
    ).toBe('open');
  });

  it('uses a stable hash-bound idempotency key', () => {
    expect(dashboardCreationIdempotencyKey('draft-1', 'abcdef0123456789zzz')).toBe(
      'dashboard-draft-draft-1-abcdef0123456789',
    );
  });

  it('normalizes warnings and the tested time range', () => {
    const warning = { code: 'EMPTY_RESULT', path: '/panels/0', message: 'No rows' };
    const value = draft({
      warnings: [warning],
      preflight: {
        warnings: [warning],
        issues: [],
        panels: [
          {
            path: '/panels/0',
            title: 'Errors',
            queryKind: 'promql',
            status: 'empty',
            testedFrom: 100,
            testedTo: 300,
            returnedRows: 0,
            scannedRows: 4,
            tookMs: 2,
          },
          {
            path: '/panels/1',
            title: 'Latency',
            queryKind: 'promql',
            status: 'passed',
            testedFrom: 50,
            testedTo: 250,
            returnedRows: 2,
            scannedRows: 8,
            tookMs: 3,
          },
        ],
      },
    });
    expect(uniqueDraftWarnings(value)).toEqual([warning]);
    expect(preflightRange(value)).toEqual({ from: 50, to: 300 });
  });
});
