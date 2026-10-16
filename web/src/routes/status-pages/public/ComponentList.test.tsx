import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import type { PublicStatusPageComponent, PublicStatusPageSnapshot } from '@/api/statusPages';
import i18n from '@/i18n';

import { PublicComponentList } from './ComponentList';

const NOW = Date.UTC(2026, 9, 16, 8) * 1_000;

function snapshot(components: PublicStatusPageComponent[]): PublicStatusPageSnapshot {
  return {
    page: {
      name: 'Acme',
      slug: 'acme',
      logo_url: null,
      brand_color: '#4F46E5',
      timezone: 'UTC',
      language: 'en-us',
      languages: ['en-us'],
      history_days: 90,
      visibility: 'public',
    },
    overall_status: 'operational',
    components,
    component_status_events: components
      .filter((component) => component.kind !== 'group')
      .map((component) => ({
        component_id: component.id,
        status: component.status,
        started_at: NOW - 30 * 24 * 60 * 60 * 1_000_000,
        ended_at: null,
      })),
    active_incidents: [],
    scheduled_maintenance: [],
    history: [],
    generated_at: NOW,
  };
}

function component(
  id: string,
  overrides: Partial<PublicStatusPageComponent> = {},
): PublicStatusPageComponent {
  return { id, name: id, description: '', status: 'operational', kind: 'component', group_id: null, ...overrides };
}

function renderList(components: PublicStatusPageComponent[]) {
  const copy = i18n.getFixedT('en-us', 'status-pages');
  render(<PublicComponentList snapshot={snapshot(components)} timezone="UTC" language="en-us" copy={copy} />);
}

describe('PublicComponentList', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
  });

  afterEach(() => cleanup());

  it('folds a group whose components are all operational into one row, and unfolds it on request', async () => {
    const user = userEvent.setup();
    renderList([
      component('ai', { kind: 'group', name: 'AI SRE', description: 'Agents that work incidents.' }),
      component('chat', { name: 'Web Chat', group_id: 'ai' }),
      component('im', { name: 'IM Chat', group_id: 'ai' }),
      component('api', { name: 'Public API' }),
    ]);

    const toggle = screen.getByRole('button', { name: /2 components/ });
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    // Folded, the group is one strip and its components are not listed.
    expect(screen.getByRole('img', { name: /^AI SRE/ })).toBeTruthy();
    expect(screen.queryByRole('heading', { name: 'Web Chat' })).toBeNull();
    expect(screen.getByRole('button', { name: 'About AI SRE' })).toBeTruthy();

    await user.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByRole('heading', { name: 'Web Chat' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'IM Chat' })).toBeTruthy();
  });

  it('opens a group unfolded when one of its components is not operational, and shows the worst', () => {
    renderList([
      component('ai', { kind: 'group', name: 'AI SRE' }),
      component('chat', { name: 'Web Chat', group_id: 'ai' }),
      component('sandbox', { name: 'Sandbox', group_id: 'ai', status: 'partial_outage' }),
    ]);

    expect(screen.getByRole('button', { name: /2 components/ }).getAttribute('aria-expanded')).toBe('true');
    const sandbox = screen.getByRole('heading', { name: 'Sandbox' }).closest('li') as HTMLElement;
    expect(within(sandbox).getByText('Partial outage')).toBeTruthy();
    expect(document.querySelector('[data-status-group]')?.getAttribute('data-status-component')).toBe('partial_outage');
  });

  it('gives every strip its own scale under it, with the uptime between its two ends', () => {
    renderList([component('api', { name: 'Public API' })]);

    const scale = document.querySelector('[data-uptime-scale]') as HTMLElement;
    expect(scale.textContent).toContain('90 days ago');
    expect(scale.textContent).toContain('100% uptime');
    expect(scale.textContent).toContain('Today');
  });
});
