import '@/i18n';

import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import * as React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';

import { StreamCombobox } from './StreamCombobox';

const STREAMS = ['access_logs', 'app_logs', 'audit'];

function renderPicker(props: Partial<React.ComponentProps<typeof StreamCombobox>> = {}) {
  const onChange = vi.fn<(stream: string) => void>();

  function Harness() {
    const [value, setValue] = React.useState(props.value ?? '');

    return (
      <StreamCombobox
        streams={STREAMS}
        {...props}
        value={value}
        onChange={(stream) => {
          setValue(stream);
          onChange(stream);
        }}
      />
    );
  }
  render(<Harness />);

  return { onChange, user: userEvent.setup() };
}

const trigger = () => screen.getByRole('combobox', { expanded: false });
const options = () => screen.queryAllByRole('option').map((option) => option.textContent);

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  Element.prototype.scrollIntoView = vi.fn();
});

afterEach(cleanup);

describe('StreamCombobox', () => {
  it('shows the stream that is chosen, or says what there is to do', () => {
    renderPicker({ value: 'app_logs' });
    expect(trigger().textContent).toBe('app_logs');
    cleanup();

    renderPicker();
    expect(trigger().textContent).toBe('Select a data stream…');
    cleanup();

    renderPicker({ creatable: true });
    expect(trigger().textContent).toBe('Choose a stream, or name a new one…');
    cleanup();

    renderPicker({ pending: true });
    expect(trigger().textContent).toBe('Loading data streams…');
  });

  it('lists the streams, narrows them as a name is typed, and takes the one picked', async () => {
    const { onChange, user } = renderPicker();

    await user.click(trigger());
    expect(options()).toEqual(STREAMS);

    await user.type(screen.getByPlaceholderText('Search streams…'), 'logs');
    expect(options()).toEqual(['access_logs', 'app_logs']);

    await user.click(screen.getByRole('option', { name: 'app_logs' }));

    expect(onChange).toHaveBeenCalledWith('app_logs');
    expect(trigger().textContent).toBe('app_logs');
    expect(screen.queryByRole('option')).toBeNull();
  });

  it('offers only what exists unless a new stream may be named', async () => {
    const { onChange, user } = renderPicker();

    await user.click(trigger());
    await user.type(screen.getByPlaceholderText('Search streams…'), 'brand_new');

    expect(options()).toEqual([]);
    expect(screen.getByText('No stream matches.')).not.toBeNull();
    expect(onChange).not.toHaveBeenCalled();
  });

  it('takes a name the workspace does not have yet, where one may be named', async () => {
    const { onChange, user } = renderPicker({ creatable: true });

    await user.click(trigger());
    await user.type(screen.getByPlaceholderText('Search, or type a new name…'), 'app_logs_clean');

    expect(options()).toEqual(['Use new stream "app_logs_clean"']);
    await user.click(screen.getByRole('option', { name: 'Use new stream "app_logs_clean"' }));

    expect(onChange).toHaveBeenCalledWith('app_logs_clean');
    expect(trigger().textContent).toBe('app_logs_clean');
  });

  it('does not offer as new a name that exists, or one that could not be a stream', async () => {
    const { user } = renderPicker({ creatable: true });

    await user.click(trigger());
    const search = screen.getByPlaceholderText('Search, or type a new name…');
    await user.type(search, 'audit');
    expect(options()).toEqual(['audit']);

    await user.clear(search);
    await user.type(search, 'my stream');
    expect(options()).toEqual([]);
    expect(screen.getByText('A stream name holds letters, numbers, _ - and . only.')).not.toBeNull();
  });

  it('starts over each time it is opened', async () => {
    const { user } = renderPicker();

    await user.click(trigger());
    await user.type(screen.getByPlaceholderText('Search streams…'), 'audit');
    await user.keyboard('{Escape}');
    await user.click(trigger());

    expect(screen.getByPlaceholderText<HTMLInputElement>('Search streams…').value).toBe('');
    expect(options()).toEqual(STREAMS);
  });
});
