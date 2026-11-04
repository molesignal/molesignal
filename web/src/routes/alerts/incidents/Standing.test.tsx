import '@/i18n';

import { cleanup, render, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { reduceMotionInTests } from '@/shell/readout/reduceMotionInTests';
import type { Incident } from '@/types/alerting';

import { IncidentStanding } from './Standing';

const SECOND = 1_000_000;
const NOW = Date.UTC(2026, 9, 11, 12) * 1_000;

function incident(overrides: Partial<Incident> = {}): Incident {
  return {
    id: 'incident-1',
    orgId: 'org-1',
    ruleId: 'rule-1',
    escalationPolicyId: 'policy-1',
    status: 'open',
    severity: 'warning',
    summary: 'Checkout latency is high',
    fingerprint: 'fp',
    currentStep: 0,
    currentLoop: 0,
    currentStepStartedAt: 0,
    assignees: [],
    createdAt: NOW - 600 * SECOND,
    labels: {},
    annotations: {},
    traceIds: [],
    hostIds: [],
    affectedServices: [],
    triggeringQuery: null,
    ...overrides,
  };
}

let restoreMotion: () => void;

beforeEach(() => {
  restoreMotion = reduceMotionInTests();
});

afterEach(() => {
  cleanup();
  restoreMotion();
});

function standing() {
  const element = document.querySelector<HTMLElement>('[data-incident-standing]');
  if (!element) throw new Error('The statement is not on the page');
  return element;
}

describe('IncidentStanding', () => {
  it('takes the colour of the worst incident that is firing and says how many are', () => {
    const active = [
      incident({ id: 'a', severity: 'critical', affectedServices: ['checkout'] }),
      incident({ id: 'b', severity: 'warning', status: 'acknowledged', labels: { service: 'cart' } }),
    ];
    render(
      <IncidentStanding
        active={active}
        waiting={[active[0]!]}
        recovered={[]}
        ruleCount={12}
        nowMicros={NOW}
      />,
    );

    expect(standing().getAttribute('data-incident-standing')).toBe('critical');
    const heading = within(standing()).getByRole('heading');
    expect(heading.className).toContain('text-red');
    expect(heading.textContent).toMatch(/2/);
    // Which services, not only how many: the names are what a reader acts on.
    expect(standing().textContent).toContain('checkout, cart');
    expect(
      within(standing()).getByRole('img').getAttribute('aria-label'),
    ).toMatch(/Critical 1, Error 0, Warning 1, Info 0$/);
  });

  it('says nothing is firing in green, and what is being watched', () => {
    render(
      <IncidentStanding
        active={[]}
        waiting={[]}
        recovered={[incident({ status: 'resolved', resolvedAt: NOW - 3_600 * SECOND })]}
        ruleCount={12}
        nowMicros={NOW}
      />,
    );

    expect(standing().getAttribute('data-incident-standing')).toBe('quiet');
    expect(within(standing()).getByRole('heading').className).toContain('text-green');
    expect(standing().textContent).toContain('12');
    // Nothing is divided when nothing is firing.
    expect(within(standing()).queryByRole('img')).toBeNull();
    expect(standing().textContent).toContain('1 hour ago');
  });
});
