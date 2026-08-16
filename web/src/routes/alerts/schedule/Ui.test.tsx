import { render } from '@testing-library/react';
import { CalendarClock } from 'lucide-react';
import { describe, expect, it } from 'vitest';

import { ScheduleCard, ScheduleSummaryCard } from './Ui';

describe('Schedule cardless surfaces', () => {
  it('renders summary metrics without an outer card or focus ring', () => {
    const { container } = render(
      <ScheduleSummaryCard
        icon={CalendarClock}
        label="Schedules"
        value="3"
        hint="Two teams"
        onClick={() => undefined}
      />,
    );

    const summary = container.querySelector('[data-schedule-summary]');
    expect(summary?.className).not.toMatch(/rounded|shadow|border|ring/);
    expect(summary?.className).toContain('focus-visible:bg-bg-2');
  });

  it('renders detail sections with only a header divider', () => {
    const { container } = render(
      <ScheduleCard title="Coverage">Schedule content</ScheduleCard>,
    );

    const section = container.querySelector('[data-schedule-section]');
    expect(section?.className).not.toMatch(/rounded|shadow|border/);
    expect(section?.querySelector('header')?.className).toContain('border-b');
  });
});
