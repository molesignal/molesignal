import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ApmMeta } from '@/api/apm';
import i18n from '@/i18n';

import { DataQualityNotice } from './DataQualityNotice';

const NOW_MICROS = Date.UTC(2026, 6, 30, 12) * 1_000;

function meta(overrides: Partial<ApmMeta> = {}): ApmMeta {
  return {
    range: { from: NOW_MICROS - 3_600_000_000, to: NOW_MICROS },
    resolution: 'minute',
    projection_started_at: NOW_MICROS - 1_800_000_000,
    last_complete_bucket_at: NOW_MICROS - 30_000_000,
    activation_boundary: false,
    data_quality: {
      partial: false,
      gaps: [],
      overflow_dimensions: [],
    },
    ...overrides,
  };
}

beforeEach(async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(NOW_MICROS / 1_000));
  await i18n.changeLanguage('en-us');
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('DataQualityNotice', () => {
  it('stays absent for current complete data', () => {
    const { container } = renderNotice(meta());
    expect(container.childElementCount).toBe(0);
  });

  it('makes projection gaps and overflow explicit', () => {
    renderNotice(
      meta({
        data_quality: {
          partial: true,
          gaps: [
            {
              org_id: 'org-a',
              range: { start: 1, end: 2 },
              reason: 'queue_full',
              dropped_facts: 3,
              recorded_at: 3,
            },
          ],
          overflow_dimensions: ['transaction'],
        },
      }),
    );
    expect(screen.getByText('Partial APM data')).toBeTruthy();
    expect(screen.getByText('transaction')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'View missing data' }));

    expect(screen.getByText('Projection queue full')).toBeTruthy();
    expect(
      screen.getByText(
        (_, element) =>
          element?.tagName === 'LI' &&
          element.textContent?.includes('3 Span facts dropped') === true,
      ),
    ).toBeTruthy();
    expect(
      screen
        .getByRole('link', { name: /Check collection setup/ })
        .getAttribute('href'),
    ).toBe('/datasource/applications/opentelemetry?signal=traces');
  });

  it('shows the activation boundary before the first complete historical window', () => {
    renderNotice(
      meta({
        activation_boundary: true,
      }),
    );
    expect(screen.getByText('APM data starts inside this window')).toBeTruthy();
  });

  it('shows stale complete-bucket state separately from partial data', () => {
    renderNotice(
      meta({
        last_complete_bucket_at: NOW_MICROS - 5 * 60_000_000,
      }),
    );
    expect(screen.getByText('APM aggregates are delayed')).toBeTruthy();
  });

  it('can be dismissed for the current page session', () => {
    renderNotice(meta({ activation_boundary: true }));

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss for now' }));

    expect(screen.queryByTestId('apm-data-quality')).toBeNull();
  });
});

function renderNotice(value: ApmMeta) {
  return render(
    <MemoryRouter>
      <DataQualityNotice meta={value} />
    </MemoryRouter>,
  );
}
