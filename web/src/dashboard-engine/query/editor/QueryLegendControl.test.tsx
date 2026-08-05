import '@/i18n';

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  DEFAULT_QUERY_LEGEND_TEMPLATE,
  QUERY_LEGEND_AUTO,
} from '../legend';
import { QueryLegendControl } from './QueryLegendControl';

afterEach(cleanup);

describe('QueryLegendControl', () => {
  it('offers Grafana Auto, Verbose, and Custom modes', () => {
    const onChange = vi.fn();
    render(
      <QueryLegendControl
        value={QUERY_LEGEND_AUTO}
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole('combobox', { name: 'Legend mode' }));
    expect(screen.getAllByText('Auto')).toHaveLength(2);
    expect(screen.getByText('Verbose')).toBeTruthy();
    expect(screen.getByText('Custom')).toBeTruthy();
    expect(screen.getByText('Only includes unique labels')).toBeTruthy();
    expect(screen.getByText('All label names and values')).toBeTruthy();
    expect(screen.getByText('Provide a naming template')).toBeTruthy();

    fireEvent.click(screen.getByText('Verbose'));
    expect(onChange).toHaveBeenLastCalledWith(undefined);
  });

  it('opens a selected Custom template and returns to Auto when cleared', () => {
    const onChange = vi.fn();
    const view = render(
      <QueryLegendControl
        value={QUERY_LEGEND_AUTO}
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole('combobox', { name: 'Legend mode' }));
    fireEvent.click(screen.getByText('Custom'));
    expect(onChange).toHaveBeenLastCalledWith(
      DEFAULT_QUERY_LEGEND_TEMPLATE,
    );

    view.rerender(
      <QueryLegendControl
        value={DEFAULT_QUERY_LEGEND_TEMPLATE}
        onChange={onChange}
      />,
    );
    const input = screen.getByRole('textbox', { name: 'Custom legend' });
    expect((input as HTMLInputElement).value).toBe(
      DEFAULT_QUERY_LEGEND_TEMPLATE,
    );

    fireEvent.change(input, { target: { value: '{{service}}' } });
    expect(onChange).toHaveBeenLastCalledWith('{{service}}');
    fireEvent.change(input, { target: { value: '' } });
    expect(onChange).toHaveBeenLastCalledWith(QUERY_LEGEND_AUTO);
  });
});
