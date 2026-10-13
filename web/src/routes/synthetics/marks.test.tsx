import '@/i18n';

import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { runs } from './__tests__/builders';
import { CheckStateMark, LocationHealthMark, OutcomeMark, RunStrip } from './marks';

afterEach(cleanup);

describe('state marks', () => {
  it('says a check is passing or failing, and a run passed or failed', () => {
    render(
      <>
        <CheckStateMark state="healthy" />
        <CheckStateMark state="failing" />
        <OutcomeMark outcome="healthy" />
        <OutcomeMark outcome="failing" />
      </>,
    );

    expect(screen.getByText('Passing')).toBeTruthy();
    expect(screen.getByText('Failing').parentElement?.className).toContain('text-red');
    expect(screen.getByText('Passed')).toBeTruthy();
    expect(screen.getByText('Failed')).toBeTruthy();
  });

  it('says a check off schedule is paused or a draft, whatever its last state was', () => {
    render(
      <>
        <CheckStateMark state="failing" lifecycle="paused" />
        <CheckStateMark state="healthy" lifecycle="draft" />
      </>,
    );

    expect(screen.getByText('Paused').parentElement?.className).not.toContain('text-red');
    expect(screen.getByText('Draft')).toBeTruthy();
    expect(screen.queryByText('Failing')).toBeNull();
    expect(screen.queryByText('Passing')).toBeNull();
  });

  it('says a location is online or not: a place does not pass', () => {
    render(
      <>
        <LocationHealthMark health="online" />
        <LocationHealthMark health="degraded" />
        <LocationHealthMark health="offline" />
      </>,
    );

    expect(screen.getByText('Online').parentElement?.className).toContain('text-green');
    expect(screen.getByText('Degraded')).toBeTruthy();
    expect(screen.getByText('Offline').parentElement?.className).toContain('text-red');
    expect(screen.queryByText('Passed')).toBeNull();
  });

  it('uses colours that hold up as text on a light surface', () => {
    const { container } = render(
      <>
        <CheckStateMark state="flaky" />
        <OutcomeMark outcome="degraded" />
      </>,
    );

    // The soft variants of the status colours fail contrast as text.
    expect(container.innerHTML).not.toMatch(/text-\w+-soft/);
  });
});

describe('RunStrip', () => {
  it('draws one tick a run, the oldest on the left, and says the count in words', () => {
    // Latest first: the newest run failed.
    const { container } = render(<RunStrip results={runs(['failing', 'healthy', 'healthy'])} length={5} />);

    const strip = screen.getByRole('img', { name: '2 of the last 3 runs passed' });
    const ticks = Array.from(strip.querySelectorAll('[data-run]')).map((tick) =>
      tick.getAttribute('data-run'),
    );
    expect(ticks).toEqual(['healthy', 'healthy', 'failing']);
    // The strip keeps its length, so the latest run is always in the same place.
    expect(strip.children).toHaveLength(5);
    expect(container.querySelectorAll('[data-run-strip]')).toHaveLength(1);
  });

  it('leaves test runs out and says so when there is nothing to draw', () => {
    render(<RunStrip results={runs(['failing'], { is_test: true })} />);

    expect(screen.queryByRole('img')).toBeNull();
    expect(screen.getByText('No runs yet')).toBeTruthy();
  });
});
