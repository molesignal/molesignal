import { cleanup, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { FlameArt, TraceArt, VitalsArt } from './art';
import { FirstRun } from './FirstRun';

const COPY = {
  eyebrow: 'Application performance',
  title: 'No APM data yet',
  description: 'Send backend traces to start building application aggregates.',
  connect: 'Connect a service',
  step: 'Instrument a service',
  stepDetail: 'Add the OpenTelemetry SDK.',
} as const;

afterEach(() => {
  cleanup();
});

describe('FirstRun', () => {
  it('says what the page is waiting for and offers the next step', () => {
    render(
      <FirstRun
        eyebrow={COPY.eyebrow}
        title={COPY.title}
        description={COPY.description}
        actions={<a href="/datasource">{COPY.connect}</a>}
      />,
    );

    expect(screen.getByRole('heading', { level: 2, name: COPY.title })).not.toBeNull();
    expect(screen.getByText(COPY.eyebrow)).not.toBeNull();
    expect(screen.getByText(COPY.description)).not.toBeNull();
    expect(screen.getByRole('link', { name: COPY.connect }).getAttribute('href')).toBe('/datasource');
    // Nothing is drawn or listed that was not asked for.
    expect(document.querySelector('[data-first-run-art]')).toBeNull();
    expect(screen.queryByRole('list')).toBeNull();
  });

  it('numbers the steps from here to a working page', () => {
    render(
      <FirstRun
        title={COPY.title}
        steps={[{ title: COPY.step, description: COPY.stepDetail }, { title: COPY.connect }]}
      />,
    );

    const steps = within(screen.getByRole('list')).getAllByRole('listitem');
    expect(steps).toHaveLength(2);
    expect(steps[0]?.textContent).toBe(`1${COPY.step}${COPY.stepDetail}`);
    expect(steps[1]?.textContent).toBe(`2${COPY.connect}`);
  });

  it('keeps the drawing out of the accessible page', () => {
    render(<FirstRun title={COPY.title} illustration={<TraceArt />} />);

    const art = document.querySelector('[data-first-run-art]');
    expect(art?.getAttribute('aria-hidden')).toBe('true');
    expect(art?.querySelector('svg')).not.toBeNull();
  });

  it('draws each module from theme tokens, not fixed colours', () => {
    for (const Art of [TraceArt, VitalsArt, FlameArt]) {
      const { container, unmount } = render(<Art />);
      const paints = Array.from(container.querySelectorAll('[fill], [stroke]')).flatMap((node) => [
        node.getAttribute('fill'),
        node.getAttribute('stroke'),
      ]);
      const colours = paints.filter((paint): paint is string => Boolean(paint) && paint !== 'none');

      expect(colours.length).toBeGreaterThan(0);
      expect(colours.every((paint) => paint.startsWith('var(--'))).toBe(true);
      unmount();
    }
  });
});
