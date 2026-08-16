import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { ValidationSummary, WorkbenchSection } from './Surfaces';

describe('Alert rule workbench surfaces', () => {
  it('uses flat sections with a single header divider', () => {
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

  it('keeps validation flat without row dividers or an outer card', () => {
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
});
