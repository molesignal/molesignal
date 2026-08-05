import '@/i18n';

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { VisualizationJsonEditor } from './VisualizationJsonEditor';

afterEach(cleanup);

describe('VisualizationJsonEditor', () => {
  it('exposes Grafana-compatible List, Table, and Hidden legend modes', () => {
    const onChange = vi.fn();
    render(
      <VisualizationJsonEditor
        options={{ legendMode: 'table' }}
        onChange={onChange}
      />,
    );

    expect(
      screen.getByRole('radio', { name: 'Table' }).getAttribute('aria-checked'),
    ).toBe('true');
    expect(screen.getAllByRole('radio')).toHaveLength(3);

    fireEvent.click(screen.getByRole('radio', { name: 'Hidden' }));
    expect(onChange).toHaveBeenCalledWith({ legendMode: 'hidden' });
  });

  it('edits legend calculations with a Grafana-style multi-select', () => {
    const onChange = vi.fn();
    render(
      <VisualizationJsonEditor
        options={{ legendStats: ['last', 'mean'] }}
        onChange={onChange}
      />,
    );

    expect(screen.getByText('Legend values')).toBeTruthy();
    expect(screen.getByText('Last')).toBeTruthy();
    expect(screen.getByText('Mean')).toBeTruthy();
    expect(screen.queryByDisplayValue('last, mean')).toBeNull();

    fireEvent.click(screen.getByRole('combobox', { name: /Legend values/ }));
    expect(screen.getAllByRole('option')).toHaveLength(5);
    expect(screen.getByRole('option', { name: 'Total' })).toBeTruthy();

    fireEvent.click(screen.getByRole('option', { name: 'Max' }));
    expect(onChange).toHaveBeenCalledWith({
      legendStats: ['last', 'max', 'mean'],
    });

    fireEvent.click(screen.getByRole('button', { name: 'Remove Mean' }));
    expect(onChange).toHaveBeenCalledWith({ legendStats: ['last'] });
  });

  it('preserves imported structured options without exposing JSON input', () => {
    render(
      <VisualizationJsonEditor
        options={{ pluginExtension: { nested: true } }}
        onChange={vi.fn()}
      />,
    );

    expect(
      screen.getByText('Imported structured option is preserved'),
    ).toBeTruthy();
    expect(screen.queryByDisplayValue(/"nested"/)).toBeNull();
  });
});
