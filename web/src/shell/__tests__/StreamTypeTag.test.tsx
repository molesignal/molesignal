import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import type { StreamType } from '@/api/streams';
import i18n from '@/i18n';

import { StreamTypeTag } from '../StreamTypeTag';

afterEach(cleanup);

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

describe('StreamTypeTag', () => {
  it.each([
    ['logs', 'Logs'],
    ['metrics', 'Metrics'],
    ['traces', 'Traces'],
    ['profiles', 'Profiles'],
    ['extend', 'Extend tables'],
  ] as const)('names a %s stream with the product name', (type, label) => {
    render(<StreamTypeTag type={type} />);
    expect(screen.getByText(label)).not.toBeNull();
  });

  it('shows the given label instead of the product name', () => {
    render(<StreamTypeTag type="logs" label="Event logs" />);
    expect(screen.getByText('Event logs')).not.toBeNull();
    expect(screen.queryByText('Logs')).toBeNull();
  });

  it('reads a type it does not know as an extension table instead of failing', () => {
    // A newer backend, or a payload without the field, must not take the page down.
    render(<StreamTypeTag type={'rum' as StreamType} />);
    expect(screen.getByText('Extend tables')).not.toBeNull();

    cleanup();
    render(<StreamTypeTag type={undefined as unknown as StreamType} />);
    expect(screen.getByText('Extend tables')).not.toBeNull();
  });
});
