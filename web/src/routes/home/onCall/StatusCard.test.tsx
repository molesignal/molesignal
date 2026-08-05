import {
  cleanup,
  fireEvent,
  render,
  screen,
} from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import type { UserLite } from '@/shell/useUsers';

import type {
  FeaturedOnCall,
  OnCallShiftOverview,
} from './model';
import { OnCallStatusCard } from './StatusCard';

const NOW = Date.UTC(2026, 6, 28, 4) * 1000;

const users = new Map<string, UserLite>([
  [
    'me',
    {
      id: 'me',
      name: 'root',
      email: 'root@example.com',
    },
  ],
  [
    'next',
    {
      id: 'next',
      name: 'Alex',
      email: 'alex@example.com',
    },
  ],
]);

const feature: FeaturedOnCall = {
  schedule: {
    id: 'schedule-1',
    org_id: 'org-1',
    name: 'Primary rota',
    description: '',
    team_id: null,
    timezone: 'UTC',
    enabled: true,
    rotations: [],
    overrides: [],
    created_at: NOW,
    updated_at: NOW,
  },
  status: 'active',
  current: {
    userId: 'me',
    source: 'rotation',
  },
  nextAt: NOW + 18 * 60 * 60 * 1_000_000,
  currentStartedAt: NOW - 6 * 60 * 60 * 1_000_000,
  nextUserId: 'next',
  activeOverride: null,
  replacedUserId: null,
  isMine: true,
};

afterEach(cleanup);

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

function renderCard(
  pendingCount: number,
  options: {
    onViewSchedule?: () => void;
    onOpenIncidents?: () => void;
    teamName?: string;
  } = {},
) {
  const shiftOverview: OnCallShiftOverview = {
    incidentCount: pendingCount,
    pendingCount,
    acknowledgedCount: 0,
    escalatedCount: 0,
    escalationPolicyNames: ['Default escalation'],
  };
  return render(
    <OnCallStatusCard
      feature={feature}
      teamName={options.teamName}
      usersById={users}
      shiftOverview={shiftOverview}
      nowMicros={NOW}
      locale="en-us"
      loading={false}
      onViewSchedule={options.onViewSchedule ?? vi.fn()}
      onViewEscalations={vi.fn()}
      onOpenIncidents={options.onOpenIncidents ?? vi.fn()}
      onArrange={vi.fn()}
      arrangeDisabled={false}
    />,
  );
}

describe('OnCallStatusCard', () => {
  it('presents the current duty as a status instead of a generic metric card', () => {
    renderCard(0);

    expect(screen.getByText('On call now')).not.toBeNull();
    expect(screen.getByText('Active')).not.toBeNull();
    expect(screen.getByText('root')).not.toBeNull();
    expect(screen.getByText('No team')).not.toBeNull();
    expect(screen.queryByText('You are currently on call')).toBeNull();
    expect(screen.getByText('Time remaining')).not.toBeNull();
    expect(screen.getByText('Current shift overview')).not.toBeNull();
    expect(screen.getByText('On duty')).not.toBeNull();
    expect(screen.getByText('Shift events')).not.toBeNull();
    expect(screen.getByText('Acknowledged')).not.toBeNull();
    expect(screen.getByText('Escalated')).not.toBeNull();
    expect(screen.getByText('Primary rota')).not.toBeNull();
    expect(screen.getByText('Default escalation')).not.toBeNull();
    expect(screen.getAllByText(/^0$/)).toHaveLength(4);
    expect(screen.queryByText('Open incidents')).toBeNull();
    expect(screen.queryByText('Current shift progress')).toBeNull();

    const viewSchedule = screen.getByRole('button', {
      name: 'View schedule',
    });
    expect(viewSchedule.className).not.toContain('border');
    expect(viewSchedule.className).not.toContain('bg-');
    expect(viewSchedule.className).toContain('-mr-1');

    expect(
      screen.queryByTestId('on-call-acknowledgement-status'),
    ).toBeNull();
    expect(screen.queryByTestId('on-call-shift-progress')).toBeNull();
  });

  it('shows the assigned team below the current user', () => {
    renderCard(0, { teamName: 'Platform operations' });

    expect(screen.getByText('Platform operations')).not.toBeNull();
    expect(screen.queryByText('No team')).toBeNull();
  });

  it('promotes pending acknowledgement as a readable warning', () => {
    renderCard(3);

    expect(
      screen.getByText('3 events need acknowledgement'),
    ).not.toBeNull();
    const acknowledgement = screen.getByTestId(
      'on-call-acknowledgement-status',
    );
    expect(acknowledgement.className).toContain('bg-orange-dim');
  });

  it('only shows the incident action when events need acknowledgement', () => {
    const onOpenIncidents = vi.fn();
    const onViewSchedule = vi.fn();
    renderCard(3, { onOpenIncidents, onViewSchedule });

    fireEvent.click(
      screen.getByRole('button', { name: 'Open incidents' }),
    );
    fireEvent.click(
      screen.getByRole('button', { name: 'View schedule' }),
    );

    expect(onOpenIncidents).toHaveBeenCalledOnce();
    expect(onViewSchedule).toHaveBeenCalledOnce();
  });
});
