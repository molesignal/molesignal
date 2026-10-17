import { useTranslation } from 'react-i18next';

import { Checkbox } from '@/shell/ui/checkbox';

import type { PipelineResources } from './resources';
import { StreamField } from './StreamField';
import type { PipelineGraphModel } from '../PipelineGraph/model';

/**
 * A stream a pipeline reads. A scheduled pipeline reads streams that exist,
 * each from where its last run left off; a realtime one names the stream it
 * listens on, which may not exist yet.
 */
export function SourceFields({
  model,
  index,
  resources,
  onChange,
}: {
  model: PipelineGraphModel;
  index: number;
  resources: PipelineResources;
  onChange: (next: PipelineGraphModel) => void;
}) {
  const { t } = useTranslation('pipelines');
  const value = model.sources[index] ?? '';
  const realtime = model.mode === 'realtime';
  const setSource = (next: string) =>
    onChange({
      ...model,
      sources: model.sources.map((item, at) => (at === index ? next : item)),
    });

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
          onChange={setSource}
        />
      </div>
      {realtime && (
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
