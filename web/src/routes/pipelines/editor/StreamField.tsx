import { useTranslation } from 'react-i18next';

import { FormField } from '@/shell/FormDrawer';

import type { PipelineResources } from './resources';
import { StreamCombobox } from './StreamCombobox';
import type { PipelineSignalType } from '../PipelineGraph/model';
import { pipelineStreamOptions } from '../PipelineGraph/streamOptions';

/**
 * One of the workspace's streams of the pipeline's signal. Where `creatable`,
 * a name the workspace does not have yet can be typed instead.
 */
export function StreamField({
  label,
  hint,
  value,
  signalType,
  resources,
  creatable = false,
  onChange,
}: {
  label: string;
  hint?: string;
  value: string;
  signalType: PipelineSignalType;
  resources: PipelineResources;
  creatable?: boolean;
  onChange: (stream: string) => void;
}) {
  const { t } = useTranslation('pipelines');
  const streams = pipelineStreamOptions(resources.streams, signalType, value);

  return (
    <FormField label={label} {...(hint ? { hint } : {})}>
      <StreamCombobox
        value={value}
        streams={streams}
        creatable={creatable}
        pending={resources.streamsPending}
        onChange={onChange}
      />
      {resources.streamsFailed && (
        <span className="font-sans text-xs leading-relaxed text-red-soft">{t('graph.stream_load_error')}</span>
      )}
    </FormField>
  );
}
