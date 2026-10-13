import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import type { TopFunction } from '@/viz/profiles/flamebearer';

import { ProfileSummary } from './Summary';

const HOT = {
  name: 'zstd::stream::raw::Encoder::run',
  selfPct: 18.46,
  totalPct: 42.25,
} as TopFunction;

function renderSummary(props: Partial<React.ComponentProps<typeof ProfileSummary>> = {}) {
  const onSelectHotFunction = vi.fn();
  const onOpenProfiles = vi.fn();
  render(
    <ProfileSummary
      typeLabel="CPU"
      totalValue="31.85 s"
      sampleCount={10_300}
      serviceCount={3}
      duration="24 min"
      selectedCount={22}
      profileCount={24}
      hotFunction={HOT}
      onSelectHotFunction={onSelectHotFunction}
      onOpenProfiles={onOpenProfiles}
      {...props}
    />,
  );

  return { onSelectHotFunction, onOpenProfiles };
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(cleanup);

describe('ProfileSummary', () => {
  it('leads with the hottest function and finds it in the graph when picked', () => {
    const { onSelectHotFunction } = renderSummary();

    const hottest = screen.getByRole('button', { name: /zstd::stream::raw::Encoder::run/ });
    expect(hottest.textContent).toBe('Hottestzstd::stream::raw::Encoder::run18.5% self');
    fireEvent.click(hottest);

    expect(onSelectHotFunction).toHaveBeenCalledWith('zstd::stream::raw::Encoder::run');
  });

  it('says what was merged to find it: one type, in its own unit', () => {
    renderSummary();

    const scope = screen.getByRole('region', { name: 'What this analysis found' }).querySelector('[data-profile-scope]')!;
    expect(scope.textContent).toBe('CPU 31.85 s·10.3K samples·3 services·24 min captured');
  });

  it('counts one service as one', () => {
    renderSummary({ serviceCount: 1 });

    expect(screen.getByText('1 service')).toBeTruthy();
  });

  it('opens the list the graph is merged from, from the count of its profiles', () => {
    const { onOpenProfiles } = renderSummary();

    const open = screen.getByRole('button', {
      name: 'Choose the profiles in this analysis: 22 of 24 selected',
    });
    expect(open.textContent).toBe('22 / 24 profiles');
    fireEvent.click(open);

    expect(onOpenProfiles).toHaveBeenCalledTimes(1);
  });

  it('has nothing to pick before a flame graph is merged', () => {
    const { onSelectHotFunction } = renderSummary({ hotFunction: null, totalValue: null, duration: null });

    const hottest = screen.getByRole('button', { name: /No hot function/ }) as HTMLButtonElement;
    expect(hottest.disabled).toBe(true);
    fireEvent.click(hottest);
    expect(onSelectHotFunction).not.toHaveBeenCalled();
    // The type is still said; what it adds up to and how long it ran are not known yet.
    expect(document.querySelector('[data-profile-scope]')!.textContent).toBe('CPU·10.3K samples·3 services');
  });
});
