import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { FormInput, FormSelect } from '@/shell/FormDrawer';

describe('FormSelect', () => {
  it('renders an explicit empty option without passing an empty value to Radix', () => {
    render(
      <FormSelect
        value=""
        onChange={vi.fn()}
        options={[
          { value: '', label: 'Use default' },
          { value: 'custom', label: 'Custom template' },
        ]}
      />,
    );

    const trigger = screen.getByRole('combobox');
    expect(trigger.textContent).toContain('Use default');
    expect(trigger.firstElementChild?.className).toContain('whitespace-nowrap');
  });

  it('keeps inputs within the width of compact form columns', () => {
    render(<FormInput aria-label="Duration" type="number" />);

    const input = screen.getByRole('spinbutton', { name: 'Duration' });
    expect(input.className).toContain('min-w-0');
    expect(input.className).toContain('w-full');
  });
});
