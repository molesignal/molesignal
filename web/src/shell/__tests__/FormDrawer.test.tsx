import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { FormInput, FormSelect, FormTextarea } from '@/shell/FormDrawer';
import { Textarea } from '@/shell/ui/textarea';

describe('FormSelect', () => {
  afterEach(cleanup);

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

  it('keeps a value that arrives in a form before its option does', () => {
    const onChange = vi.fn();
    const tree = (value: string, options: Array<{ value: string; label: string }>) => (
      <form>
        <FormSelect
          value={value}
          onChange={onChange}
          options={[{ value: '', label: 'Pick one' }, ...options]}
        />
      </form>
    );

    const { rerender } = render(tree('', []));
    rerender(tree('saved', []));
    rerender(tree('saved', [{ value: 'saved', label: 'The saved one' }]));

    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole('combobox').textContent).toContain('The saved one');
  });

  it('still lets someone pick the empty option in a form', () => {
    const onChange = vi.fn();

    render(
      <form>
        <FormSelect
          value="saved"
          onChange={onChange}
          options={[
            { value: '', label: 'Pick one' },
            { value: 'saved', label: 'The saved one' },
          ]}
        />
      </form>,
    );

    const native = document.querySelector('select') as HTMLSelectElement;
    const empty = Array.from(native.options).find((option) => option.textContent === 'Pick one');
    fireEvent.change(native, { target: { value: empty?.value } });

    expect(onChange).toHaveBeenCalledWith('');
  });

  it('keeps inputs within the width of compact form columns', () => {
    render(<FormInput aria-label="Duration" type="number" />);

    const input = screen.getByRole('spinbutton', { name: 'Duration' });
    expect(input.className).toContain('min-w-0');
    expect(input.className).toContain('w-full');
  });

  it('uses Control Surface styling for form controls', () => {
    render(
      <>
        <FormInput aria-label="Name" />
        <FormTextarea aria-label="Description" />
        <FormSelect
          ariaLabel="Kind"
          value="http"
          onChange={vi.fn()}
          options={['http']}
        />
        <Textarea aria-label="Notes" />
      </>,
    );

    for (const control of [
      screen.getByRole('textbox', { name: 'Name' }),
      screen.getByRole('textbox', { name: 'Description' }),
      screen.getByRole('combobox', { name: 'Kind' }),
      screen.getByRole('textbox', { name: 'Notes' }),
    ]) {
      expect(control.className).toContain('border-0');
      expect(control.className).toContain('bg-[var(--control-surface)]');
    }
  });
});
