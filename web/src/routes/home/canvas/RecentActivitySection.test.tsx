import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { AuditEvent } from '@/api/audit';
import i18n from '@/i18n';

import { RecentActivitySection } from './RecentActivitySection';

const ROW_HEIGHT = 50;

function event(index: number): AuditEvent {
  return {
    id: `event-${index}`,
    org_id: 'org-1',
    actor_kind: 'user',
    actor_id: 'user-1',
    action: 'dashboard.updated',
    target_kind: 'dashboard',
    target_id: `dashboard-${index}`,
    payload: {},
    ts_micros: 1_800_000_000_000_000 - index * 60_000_000,
  };
}

function rect(top: number, bottom: number): DOMRect {
  return {
    top,
    bottom,
    left: 0,
    right: 0,
    x: 0,
    y: top,
    width: 0,
    height: bottom - top,
    toJSON: () => ({}),
  };
}

/** jsdom has no layout: stack the rows at a fixed height inside a viewport of the given height. */
function layOut(viewportHeight: number) {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(
    function (this: HTMLElement) {
      if (this.hasAttribute('data-fit-row')) {
        const index = Array.from(this.parentElement?.children ?? []).indexOf(this);
        return rect(index * ROW_HEIGHT, (index + 1) * ROW_HEIGHT);
      }
      return rect(0, viewportHeight);
    },
  );
}

function renderSection(count: number) {
  return render(
    <RecentActivitySection
      events={Array.from({ length: count }, (_, index) => event(index))}
      state={null}
      error={null}
      onViewAll={vi.fn()}
      onCreateAlert={vi.fn()}
      createAlertDisabled={false}
    />,
  );
}

const rowsOf = (container: HTMLElement) =>
  Array.from(container.querySelectorAll<HTMLElement>('[data-fit-row]'));
const connectorsOf = (container: HTMLElement) =>
  container.querySelectorAll('li > span[aria-hidden="true"]');

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

describe('RecentActivitySection', () => {
  it('shows every event when the card has room for them', () => {
    layOut(10_000);
    const { container } = renderSection(5);

    const rows = rowsOf(container);
    expect(rows).toHaveLength(5);
    expect(rows.filter((row) => row.classList.contains('invisible'))).toHaveLength(0);
    // The timeline joins each row to the next, so five rows have four joints.
    expect(connectorsOf(container)).toHaveLength(4);
  });

  it('hides the rows that do not fit instead of cutting one in half', () => {
    // Rows end at 50, 100, 150 ...: only the first two fit inside 130.
    layOut(130);
    const { container } = renderSection(5);

    const hidden = rowsOf(container).map((row) => row.classList.contains('invisible'));
    expect(hidden).toEqual([false, false, true, true, true]);
    // The timeline stops at the last visible row rather than dangling past it.
    expect(connectorsOf(container)).toHaveLength(1);
  });

  it('always keeps the newest event, even in a card too short for any row', () => {
    layOut(10);
    const { container } = renderSection(3);

    const hidden = rowsOf(container).map((row) => row.classList.contains('invisible'));
    expect(hidden).toEqual([false, true, true]);
  });

  it('offers to create an alert when there is no activity', () => {
    const { container } = render(
      <RecentActivitySection
        events={[]}
        state="empty"
        error={null}
        onViewAll={vi.fn()}
        onCreateAlert={vi.fn()}
        createAlertDisabled={false}
      />,
    );

    expect(rowsOf(container)).toHaveLength(0);
    expect(screen.getByRole('button', { name: /create/i })).not.toBeNull();
  });
});
