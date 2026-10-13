import '@/i18n';

import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import type { StreamSummary } from '@/api/streams';

import { LogStreamSelect, sizingNames } from './StreamSelect';

afterEach(cleanup);

const stream = (name: string, label = name) => ({ id: name, name, label }) as StreamSummary;

describe('sizingNames', () => {
  it('puts the longest names first, a CJK character counting for two', () => {
    expect(sizingNames(['app', 'application_logs', '网关访问日志（生产环境）', 'nginx'])).toEqual([
      // Twelve characters, drawn about as wide as twenty-four Latin letters.
      '网关访问日志（生产环境）',
      'application_logs',
      'nginx',
      'app',
    ]);
  });

  it('names each once and stops at the fifty longest', () => {
    expect(sizingNames(['app', 'app', 'nginx'])).toEqual(['nginx', 'app']);

    const many = Array.from({ length: 300 }, (_, index) => `stream_${'x'.repeat(index)}`);
    const kept = sizingNames(many);

    expect(kept).toHaveLength(50);
    expect(kept[0]).toBe(many[299]);
    expect(kept[49]).toBe(many[250]);
  });
});

describe('LogStreamSelect', () => {
  it('is sized by the names of the list without saying them', () => {
    render(
      <LogStreamSelect
        streams={[stream('app'), stream('application_logs'), stream('k8s', 'Kubernetes events')]}
        value="app"
        loading={false}
        onChange={() => {}}
      />,
    );

    const field = screen.getByRole('combobox', { name: 'Select log stream' });
    // What sizes it is drawn by CSS from an attribute: no text of the field, nothing for a reader.
    const sizers = [...field.querySelectorAll('[data-label]')];
    expect(sizers.map((sizer) => sizer.getAttribute('data-label'))).toEqual([
      'Kubernetes events',
      'application_logs',
      'app',
    ]);
    expect(sizers.every((sizer) => sizer.getAttribute('aria-hidden') === 'true')).toBe(true);
    expect(sizers.every((sizer) => sizer.textContent === '')).toBe(true);
    // It is as wide as what it holds, up to the width it had while it was fixed.
    expect(field.className.split(' ')).toEqual(expect.arrayContaining(['w-auto', 'max-w-[220px]']));
    expect(field.className.split(' ')).not.toContain('w-[220px]');
  });

  it('holds the place of the list until there is one', () => {
    const { rerender } = render(<LogStreamSelect streams={[]} value="" loading onChange={() => {}} />);
    expect(screen.getByText('Loading streams…')).toBeTruthy();
    expect(screen.queryByRole('combobox')).toBeNull();

    rerender(<LogStreamSelect streams={[]} value="" loading={false} onChange={() => {}} />);
    expect(screen.getByText('No streams available')).toBeTruthy();
  });
});
