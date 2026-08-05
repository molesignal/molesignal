import { describe, expect, it } from 'vitest';

import type { DashboardDraftPreview } from '@/api/intelligence/dashboardAuthoring';
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
    draft_id: 'draft-1',
    model_hash: 'a'.repeat(64),
    status: 'ready',
    created_at: NOW - 60_000_000,
    expires_at: NOW + 600_000_000,
    compiled_model: createEmptyDashboardDefinition('AI preview'),
    warnings: [],
    preflight: { panels: [], warnings: [], issues: [] },
    operation: null,
    dashboard_id: null,
    dashboard_route: null,
    can_propose: true,
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
            approval_id: 'approval-1',
            status: 'approved',
            required_approvals: 0,
            approved_reviews: 0,
          },
        }),
        NOW,
      ),
    ).toBe('execute');
    expect(
      dashboardDraftAction(
        draft({
          operation: {
            approval_id: 'approval-2',
            status: 'pending',
            required_approvals: 2,
            approved_reviews: 1,
          },
        }),
        NOW,
      ),
    ).toBe('wait_for_review');
  });

  it('blocks on either draft or approval expiry and opens consumed results', () => {
    const approvalExpired = draft({
      operation: {
        approval_id: 'approval-1',
        status: 'approved',
        required_approvals: 0,
        approved_reviews: 0,
        expires_at: NOW - 1,
      },
    });
    expect(effectiveDraftExpiry(approvalExpired)).toBe(NOW - 1);
    expect(dashboardDraftAction(approvalExpired, NOW)).toBe('blocked');
    expect(
      dashboardDraftAction(
        draft({
          status: 'consumed',
          dashboard_id: 'dashboard-1',
          dashboard_route: '/dashboards/dashboard-1',
          can_propose: false,
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
            query_kind: 'promql',
            status: 'empty',
            tested_from_micros: 100,
            tested_to_micros: 300,
            returned_rows: 0,
            scanned_rows: 4,
            took_ms: 2,
          },
          {
            path: '/panels/1',
            title: 'Latency',
            query_kind: 'promql',
            status: 'passed',
            tested_from_micros: 50,
            tested_to_micros: 250,
            returned_rows: 2,
            scanned_rows: 8,
            took_ms: 3,
          },
        ],
      },
    });
    expect(uniqueDraftWarnings(value)).toEqual([warning]);
    expect(preflightRange(value)).toEqual({ from: 50, to: 300 });
  });
});
