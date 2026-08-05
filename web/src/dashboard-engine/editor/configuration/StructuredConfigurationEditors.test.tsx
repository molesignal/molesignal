import '@/i18n';

import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { AnnotationEventsEditor } from './AnnotationEventsEditor';
import { OverridePropertiesEditor } from './OverridePropertiesEditor';
import { StringMapEditor } from './StringMapEditor';
import { ThresholdsEditor } from './ThresholdsEditor';
import { TransformationOptionsEditor } from './TransformationOptionsEditor';
import { ValueMappingsEditor } from './ValueMappingsEditor';

describe('structured Dashboard configuration editors', () => {
  it('edits transformation options without dropping imported keys', () => {
    const onChange = vi.fn();
    render(
      <TransformationOptionsEditor
        type="filter_fields"
        value={{ include: ['service'], providerExtension: true }}
        onChange={onChange}
      />,
    );

    fireEvent.change(screen.getByLabelText('Include fields'), {
      target: { value: 'service, region' },
    });

    expect(onChange).toHaveBeenCalledWith({
      include: ['service', 'region'],
      providerExtension: true,
    });
    expect(screen.queryByDisplayValue(/\{\s*"include"/)).toBeNull();
  });

  it('edits rename maps as key and value rows', () => {
    const onChange = vi.fn();
    render(
      <StringMapEditor
        value={{ service: 'Service name' }}
        onChange={onChange}
      />,
    );

    fireEvent.change(screen.getByLabelText('Value'), {
      target: { value: 'Workload' },
    });

    expect(onChange).toHaveBeenCalledWith({ service: 'Workload' });
  });

  it('adds annotation events while preserving provider query fields', () => {
    const onChange = vi.fn();
    render(
      <AnnotationEventsEditor
        value={{ provider: 'deployments-v2', filter: 'env=prod' }}
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Add event' }));

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: 'deployments-v2',
        filter: 'env=prod',
        items: [
          expect.objectContaining({
            id: 'event-1',
            label: 'Event 1',
            timestamp: expect.any(Number),
          }),
        ],
      }),
    );
  });

  it('edits thresholds and value mappings with typed rows', () => {
    const onThresholdChange = vi.fn();
    const { unmount } = render(
      <ThresholdsEditor
        value={{
          mode: 'absolute',
          steps: [{ value: null, color: 'var(--success)', label: 'Healthy' }],
        }}
        onChange={onThresholdChange}
      />,
    );

    fireEvent.change(screen.getByLabelText('Threshold mode'), {
      target: { value: 'percentage' },
    });
    expect(onThresholdChange).toHaveBeenCalledWith(
      expect.objectContaining({ mode: 'percentage' }),
    );
    unmount();

    const onMappingChange = vi.fn();
    render(<ValueMappingsEditor value={[]} onChange={onMappingChange} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add mapping' }));
    expect(onMappingChange).toHaveBeenCalledWith([
      { type: 'value', value: '', result: { text: '' } },
    ]);
  });

  it('preserves unsupported imported override values without showing JSON', () => {
    render(
      <OverridePropertiesEditor
        value={[
          {
            id: 'pluginExtension',
            value: { nested: true },
          },
        ]}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByText('Imported property value is preserved')).toBeTruthy();
    expect(screen.queryByDisplayValue('{"nested":true}')).toBeNull();
  });
});
