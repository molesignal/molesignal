import type * as streamsApi from '@/api/streams';

import type { PipelineSignalType } from './model';

/**
 * The names of the streams a source or destination can be, for the pipeline's
 * signal. A name already in the pipeline is kept on offer even when the
 * catalogue does not list it, so opening a pipeline never blanks its field.
 */
export function pipelineStreamOptions(
  streams: streamsApi.StreamSummary[],
  signalType: PipelineSignalType,
  currentValue = '',
): string[] {
  const names = streams
    .filter((stream) => stream.stream_type === signalType)
    .map((stream) => stream.name)
    .sort((left, right) => left.localeCompare(right));

  return currentValue && !names.includes(currentValue) ? [currentValue, ...names] : names;
}
