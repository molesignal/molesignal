import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { Readout } from './Readout';
import { reduceMotionInTests } from './reduceMotionInTests';
import { SpectrumBar } from './SpectrumBar';
import { readoutNumber, splitReadoutValue } from './value';

const COPY = {
  label: 'Error rate',
  detail: '1,895 errors',
  spread: 'Service health',
} as const;

let restoreMotion = () => {};

beforeEach(() => {
  restoreMotion = reduceMotionInTests();
});

afterEach(() => {
  cleanup();
  restoreMotion();
});

describe('readout values', () => {
  it('splits a formatted figure into the number and its unit', () => {
    expect(splitReadoutValue('46.9 req/s')).toEqual({ number: '46.9', unit: 'req/s' });
    expect(splitReadoutValue('1.12%')).toEqual({ number: '1.12', unit: '%' });
    expect(splitReadoutValue('12,840')).toEqual({ number: '12,840', unit: '' });
    expect(splitReadoutValue('—')).toEqual({ number: '—', unit: '' });
  });

  it('knows which figures can be counted up to', () => {
    expect(readoutNumber('12,840')).toEqual({ value: 12_840, decimals: 0, grouped: true });
    expect(readoutNumber('1.12')).toEqual({ value: 1.12, decimals: 2, grouped: false });
    expect(readoutNumber('—')).toBeNull();
    expect(readoutNumber('1.2.3')).toBeNull();
  });
});

describe('Readout', () => {
  it('sets the unit apart from the figure and keeps the thousands separators', () => {
    render(<Readout label={COPY.label} value="12,840 req/s" detail={COPY.detail} />);

    const cell = screen.getByText(COPY.label).closest('[data-readout]')!;
    expect(cell.textContent).toBe(`${COPY.label}12,840req/s${COPY.detail}`);
  });

  it('shows a figure that is not a number as it is', () => {
    render(<Readout label={COPY.label} value="—" />);

    expect(screen.getByText('—')).not.toBeNull();
    expect(document.querySelector('[data-sparkline]')).toBeNull();
  });

  it('gives the figure and its trend the colour of its state', () => {
    render(<Readout label={COPY.label} value="13.0%" tone="danger" trend={{ values: [1, 4, 2] }} />);

    const cell = screen.getByText(COPY.label).closest('[data-readout]')!;
    expect(cell.querySelector('.type-kpi')?.className).toContain('text-red');
    expect(cell.querySelector('[data-sparkline="line"]')?.className).toContain('text-red');
  });

  it('draws counts as bars and levels as a line', () => {
    const { rerender } = render(
      <Readout label={COPY.label} value="5" trend={{ values: [1, 4, 2], variant: 'bars' }} />,
    );
    expect(document.querySelectorAll('[data-sparkline="bars"] rect')).toHaveLength(3);

    rerender(<Readout label={COPY.label} value="5" trend={{ values: [1, 4, 2] }} />);
    expect(document.querySelector('[data-sparkline="line"] path')).not.toBeNull();
  });
});

describe('SpectrumBar', () => {
  const segments = [
    { key: 'healthy', label: 'Healthy', value: 4, color: 'var(--green)' },
    { key: 'warning', label: 'Warning', value: 0, color: 'var(--yellow)' },
    { key: 'critical', label: 'Critical', value: 1, color: 'var(--red)' },
  ];

  it('gives each part the share of the bar it has of the whole', () => {
    render(<SpectrumBar label={COPY.spread} segments={segments} />);

    const parts = Array.from(document.querySelectorAll<HTMLElement>('[data-spectrum-segment]'));
    expect(parts.map((part) => part.getAttribute('data-spectrum-segment'))).toEqual([
      'healthy',
      'critical',
    ]);
    expect(parts.map((part) => part.style.flexGrow)).toEqual(['4', '1']);
  });

  it('lists every part with its count, including the ones at zero', () => {
    render(<SpectrumBar label={COPY.spread} segments={segments} />);

    expect(screen.getAllByRole('listitem').map((item) => item.textContent)).toEqual([
      'Healthy4',
      'Warning0',
      'Critical1',
    ]);
    expect(screen.getByRole('img').getAttribute('aria-label')).toBe(
      'Service health: Healthy 4, Warning 0, Critical 1',
    );
  });

  it('is an empty track when there is nothing to divide', () => {
    render(
      <SpectrumBar
        label={COPY.spread}
        segments={segments.map((segment) => ({ ...segment, value: 0 }))}
      />,
    );

    expect(document.querySelector('[data-spectrum-segment]')).toBeNull();
    expect(screen.getByRole('img').className).toContain('bg-bd-0');
  });
});
