import { useTranslation } from 'react-i18next';

import type { ScheduledPipeline } from '@/api/pipelines';
import { ReadOnlyValue } from '@/routes/settings/_atoms';
import { FormField } from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';

import { AuditReaderNote } from './AuditReaderNote';
import type { PipelineResources } from './resources';
import { StreamField } from './StreamField';
import { changesInPlace, type PipelineGraphModel, type PipelineSignalType } from '../PipelineGraph/model';
import { AUDIT_STREAM, isRealtimeSource, isRumStream } from '../PipelineGraph/streamName';

/**
 * A stream a pipeline reads. A scheduled pipeline reads streams that exist,
 * each from where its last run left off; a realtime one names the stream it
 * listens on, which may not exist yet.
 */
export function SourceFields({
  model,
  index,
  resources,
  saved,
  canSave,
  onChange,
}: {
  model: PipelineGraphModel;
  index: number;
  resources: PipelineResources;
  /** The pipeline as it was last saved; `null` for a new one. */
  saved: ScheduledPipeline | null;
  canSave: boolean;
  onChange: (next: PipelineGraphModel) => void;
}) {
  const { t } = useTranslation('pipelines');
  const value = model.sources[index] ?? '';
  const realtime = model.mode === 'realtime';
  // Metrics and spans stay in their stream: there is no other stream to keep the original in.
  const retains = realtime && !changesInPlace(model);
  const setSource = (next: string) =>
    onChange({
      ...model,
      sources: model.sources.map((item, at) => (at === index ? next : item)),
    });

  // Every profile of an organization arrives in one stream, so there is nothing to choose.
  if (model.signalType === 'profiles') {
    return (
      <div className="max-w-[420px]" data-pipeline-profile-source>
        <FormField label={t('graph.source_name')} hint={t('editor.source_profiles_hint')}>
          <ReadOnlyValue value={value} mono />
        </FormField>
      </div>
    );
  }

  return (
    <div className="grid max-w-[640px] gap-[12px]">
      <div className="max-w-[420px]">
        <StreamField
          label={t('graph.source_name')}
          hint={t(realtime ? 'editor.source_realtime_hint' : 'editor.source_hint')}
          value={value}
          signalType={model.signalType}
          resources={resources}
          creatable={realtime}
          {...(realtime && { offers: isRealtimeSource })}
          onChange={setSource}
        />
        {value.trim() === AUDIT_STREAM && <AuditReaderNote pipeline={saved} canSave={canSave} />}
      </div>
      {retains && (
        <div>
          <label className="flex cursor-pointer items-center gap-2 font-sans text-sm text-tx-1">
            <Checkbox
              checked={model.retainSource ?? false}
              onCheckedChange={(checked) => onChange({ ...model, retainSource: checked === true })}
            />
            {t('realtime.retain')}
          </label>
          <p className="m-0 mt-1 pl-[26px] font-sans text-xs leading-relaxed text-tx-3">
            {t('realtime.retain_hint', { source: value })}
          </p>
        </div>
      )}
    </div>
  );
}

/**
 * Where metrics, spans or RUM events changed as they arrive go, back into the
 * stream they came from (`source`), and what of them a step may not change.
 */
export function InPlaceNote({ signal, source }: { signal: PipelineSignalType; source: string }) {
  const { t } = useTranslation('pipelines');
  const note =
    signal === 'metrics'
      ? 'realtime.in_place_metrics'
      : signal === 'traces'
        ? 'realtime.in_place_traces'
        : signal === 'profiles'
          ? 'realtime.in_place_profiles'
          : isRumStream(source)
          ? 'realtime.in_place_rum'
          : null;

  return note && <p className="m-0 max-w-[640px] font-sans text-sm leading-relaxed text-tx-2">{t(note)}</p>;
}
