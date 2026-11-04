import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen } from '@testing-library/react';
import type * as React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import '@/i18n';
import { DEFAULT_USER_PREFERENCES } from '@/api/me';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';

import { RumInvestigateButton } from './InvestigateButton';
import type { RumAgentSubject } from './prompt';

const access = vi.hoisted(() => ({ allowed: true }));

vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({ allowed: access.allowed, disabled: !access.allowed }),
}));

const SHOP = 'Shop';
const ROW_NAME = 'Investigate /checkout with Mole Agent';

const subject: RumAgentSubject = {
  application: { id: 'shop-web', name: SHOP },
  environment: 'prod',
  range: { from: 1_760_000_000_000_000, to: 1_760_003_600_000_000 },
  focus: { kind: 'errors' },
};

/** The entry, where the clock of the reader is read from their saved preferences. */
function renderEntry(entry: React.ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });

  client.setQueryData(['me', 'preferences'], { ...DEFAULT_USER_PREFERENCES, timezone: 'UTC' });

  return render(<QueryClientProvider client={client}>{entry}</QueryClientProvider>);
}

describe('the entry to Mole Agent from RUM', () => {
  beforeEach(() => {
    access.allowed = true;
    useMoleAgentStore.setState({ isOpen: false, intent: null });
  });

  afterEach(() => {
    useMoleAgentStore.setState({ isOpen: false, intent: null });
  });

  it('opens a chat about the application, with the question drafted and the application attached', () => {
    renderEntry(<RumInvestigateButton subject={subject} />);

    fireEvent.click(screen.getByRole('button', { name: /Mole Agent/ }));

    const { isOpen, intent } = useMoleAgentStore.getState();

    expect(isOpen).toBe(true);
    expect(intent?.subject).toEqual({ kind: 'rum', id: 'shop-web' });
    expect(intent?.references).toEqual([{ kind: 'rum_application', id: 'shop-web', label: SHOP }]);
    expect(intent?.prompt).toContain(SHOP);
    expect(intent?.prompt).toContain('errors');
  });

  it('says what it asks about in the attribute the tests and the pages read', () => {
    const { container } = renderEntry(<RumInvestigateButton subject={subject} />);

    expect(container.querySelector('[data-rum-investigate="errors"]')).not.toBeNull();
  });

  it('is an icon with a name where a row has no room for words', () => {
    renderEntry(<RumInvestigateButton subject={subject} look="icon" title={ROW_NAME} />);

    expect(screen.getByRole('button', { name: ROW_NAME })).toBeTruthy();
  });

  it('is not there for those who may not use Mole Agent', () => {
    access.allowed = false;

    const { container } = renderEntry(<RumInvestigateButton subject={subject} />);

    expect(container.firstChild).toBeNull();
  });
});
