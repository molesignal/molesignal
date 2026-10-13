import '@/i18n';

import { cleanup, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { reduceMotionInTests } from '@/shell/readout/reduceMotionInTests';

import { agent } from '../__tests__/builders';
import { AgentsStanding } from './Standing';

let restoreMotion: () => void;

beforeEach(() => {
  restoreMotion = reduceMotionInTests();
});

afterEach(() => {
  cleanup();
  restoreMotion();
});

function standing() {
  return screen.getByRole('region', { name: 'Probe agents now' });
}

describe('AgentsStanding', () => {
  it('says the fleet is online when every agent is, with the room they have left', () => {
    render(<AgentsStanding agents={[agent({ id: 'a' }), agent({ id: 'b' })]} />);

    expect(within(standing()).getByRole('heading').textContent).toBe('All 2 agents are online');
    expect(standing().getAttribute('data-agents-standing')).toBe('online');
    expect(standing().textContent).toContain('24of 32');
    expect(standing().textContent).toContain('4of 4');
  });

  it('counts the agents that are not online and leaves revoked ones out of the fleet', () => {
    render(
      <AgentsStanding
        agents={[
          agent({ id: 'a' }),
          agent({ id: 'b', status: 'offline' }),
          agent({ id: 'c', status: 'degraded' }),
          agent({ id: 'd', status: 'revoked' }),
        ]}
      />,
    );

    const heading = within(standing()).getByRole('heading');
    expect(heading.textContent).toBe('2 agents need attention');
    // One of them is down, which is worse than slow.
    expect(heading.className).toContain('text-red');
    expect(
      within(standing()).getByRole('img', { name: /Agent states/ }).getAttribute('aria-label'),
    ).toBe('Agent states: Offline 1, Degraded 1, Online 1');
  });

  it('says there is no agent instead of a row of zeroes', () => {
    render(<AgentsStanding agents={[agent({ status: 'revoked' })]} />);

    expect(within(standing()).getByRole('heading').textContent).toBe('No agent is registered');
    expect(within(standing()).queryByRole('img')).toBeNull();
    expect(standing().textContent).not.toContain('0of');
  });
});
