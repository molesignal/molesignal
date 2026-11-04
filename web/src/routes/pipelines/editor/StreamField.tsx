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
  clearable = false,
  placeholder,
  disabled = false,
  offers,
  onChange,
}: {
  label: string;
  hint?: string;
  value: string;
  signalType: PipelineSignalType;
  resources: PipelineResources;
  creatable?: boolean;
  /** No stream is a choice too; `placeholder` then says what that means. */
  clearable?: boolean;
  placeholder?: string;
  disabled?: boolean;
  /** Which of the workspace's streams are on offer here; every one when left out. */
  offers?: (stream: string) => boolean;
  onChange: (stream: string) => void;
}) {
  const { t } = useTranslation('pipelines');
  const onOffer = offers ? resources.streams.filter((stream) => offers(stream.name)) : resources.streams;
  const streams = pipelineStreamOptions(onOffer, signalType, value);

  return (
    <FormField label={label} {...(hint ? { hint } : {})}>
      <StreamCombobox
        value={value}
        streams={streams}
        creatable={creatable}
        clearable={clearable}
        placeholder={placeholder}
        pending={resources.streamsPending}
        disabled={disabled}
        onChange={onChange}
      />
      {resources.streamsFailed && (
        <span className="font-sans text-xs leading-relaxed text-red-soft">{t('graph.stream_load_error')}</span>
      )}
    </FormField>
  );
}
