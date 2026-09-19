import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import * as React from 'react';
import { describe, expect, it } from 'vitest';

import { ProfileTagInput, splitTagInput } from './TagInput';

describe('ProfileTagInput', () => {
  it('splits pasted comma and newline separated values without duplicates', () => {
    expect(splitTagInput('production, staging\nproduction')).toEqual([
      'production',
      'staging',
    ]);
  });

  it('creates removable tags with Enter and Backspace', async () => {
    const user = userEvent.setup();
    render(<Harness />);

    const input = screen.getByRole('textbox');
    await user.type(input, 'production{Enter}');
    expect(screen.getByText('production')).toBeInTheDocument();

    await user.type(input, 'staging{Enter}');
    expect(screen.getByText('staging')).toBeInTheDocument();

    await user.type(input, '{Backspace}');
    expect(screen.queryByText('staging')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Remove production' }));
    expect(screen.queryByText('production')).not.toBeInTheDocument();
  });
});

function Harness() {
  const [values, setValues] = React.useState<string[]>([]);
  return (
    <ProfileTagInput
      id="scope-values"
      describedBy="scope-values-hint"
      values={values}
      onChange={setValues}
      placeholder="Type a value"
      removeLabel={(value) => `Remove ${value}`}
    />
  );
}
