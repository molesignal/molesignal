import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import i18n from '@/i18n';

import { QueryPreview, ValidationSummary, WorkbenchSection } from './Surfaces';

describe('Alert rule workbench surfaces', () => {
  it('uses sections with a single header divider', () => {
    const { container } = render(
      <WorkbenchSection
        id="identity"
        number="01"
        title="Identity"
        description="Rule metadata"
      >
        Fields
      </WorkbenchSection>,
    );

    const section = container.querySelector('[data-alert-workbench-section]');
    expect(section?.className).not.toMatch(/rounded|shadow|border/);
    expect(section?.querySelector('header')?.className).toContain('border-b');
  });

  it('uses a compact validation section', () => {
    const { container } = render(
      <ValidationSummary
        identityReady
        queryReady={false}
        thresholdsReady
        runbookReady
      />,
    );

    const section = container.querySelector('[data-alert-validation]');
    expect(section?.className).not.toMatch(/rounded|shadow|border/);
    expect(section?.querySelector('ul')?.className).not.toContain('divide-y');
  });

  it('says how far back the preview reaches: six hours for a query, what the server read for a figure', async () => {
    await i18n.changeLanguage('en-us');

    const props = {
      pending: false,
      attempted: false,
      stale: false,
      error: null,
      points: [],
      timestamps: [],
      threshold: null,
      onRun: () => {},
    };
    const { container, rerender } = render(<QueryPreview {...props} />);
    const range = () => container.querySelector('header p')?.textContent;

    expect(range()).toBe('Real data · last 6 hours');

    // How far a figure is read back is known once the server has read it.
    rerender(<QueryPreview {...props} hours={null} />);
    expect(range()).toBe('Real data');

    rerender(<QueryPreview {...props} hours={12} />);
    expect(range()).toBe('Real data · last 12 hours');

    rerender(<QueryPreview {...props} hours={1} />);
    expect(range()).toBe('Real data · last hour');
  });
});
