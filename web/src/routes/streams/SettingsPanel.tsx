import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type * as streamsApi from '@/api/streams';
import { uiLabelClass } from '@/shell/chrome';
import { FieldArray, FormField, FormInput, FormSelect, FormTextarea } from '@/shell/FormDrawer';

import { INDEX_OPTIONS } from './fieldEditor/model';
import {
  MAX_INDEX_PATTERNS,
  MAX_PARTITION_FIELDS,
  MAX_PARTITION_SHARDS,
  PARTITION_BUCKETS,
  PREFIX_SHARDS,
  newPartition,
  totalShards,
  type PartitionDraft,
  type StreamDraft,
} from './settingsDraft';
import { StreamSettingsSection, StreamToggleRow } from './Surfaces';

/** 能用来分片的字段：没加密的文本字段，再加上已经配好、还没写入数据的。 */
function partitionFields(draft: StreamDraft): string[] {
  const fields = draft.fields
    .filter((field) => field.dataType === 'utf8' && !field.encrypted)
    .map((field) => field.name);
  for (const partition of draft.partitions) {
    if (!fields.includes(partition.field)) fields.push(partition.field);
  }
  return fields;
}

/** 一两个分片字段，每行一个字段和它的分片方式；分片总数写在标题那一行。 */
function PartitionFields({
  draft,
  onChange,
}: {
  draft: StreamDraft;
  onChange: (patch: Partial<StreamDraft>) => void;
}) {
  const { t } = useTranslation('streams');
  const labelId = React.useId();
  const fields = partitionFields(draft);
  const partitions = draft.partitions;
  const unused = fields.filter((field) => partitions.every((partition) => partition.field !== field));

  return (
    <div role="group" aria-labelledby={labelId} className="flex flex-col gap-1.5">
      <div className="flex items-center gap-2">
        <span id={labelId} className={uiLabelClass}>
          {t('explore.runtime.partition_field')}
        </span>
        <span className="ml-auto font-sans text-xs tabular-nums text-tx-3">
          {partitions.length > 0
            ? t('explore.runtime.partition_total', { count: totalShards(partitions) })
            : t('explore.runtime.partition_none')}
        </span>
      </div>
      <FieldArray<PartitionDraft>
        items={partitions}
        onChange={(next) => onChange({ partitions: next })}
        maxItems={Math.min(MAX_PARTITION_FIELDS, partitions.length + unused.length)}
        newItem={() => newPartition(unused[0] ?? '', partitions)}
        addLabel={t('explore.runtime.partition_add')}
        removeLabel={t('explore.runtime.partition_remove')}
        rowClassName="items-center"
        renderItem={(partition, index, setPartition) => {
          const others = partitions.filter((_, at) => at !== index);
          // Shards this field may split into: the other field's shards times this field's stay
          // within the cap.
          const room = MAX_PARTITION_SHARDS / totalShards(others);
          const tooMany = t('explore.runtime.partition_too_many', { max: MAX_PARTITION_SHARDS });
          return (
            <div className="flex gap-2">
              <div className="min-w-0 flex-1">
                <FormSelect
                  ariaLabel={t('explore.runtime.partition_field')}
                  value={partition.field}
                  onChange={(field) => setPartition({ ...partition, field })}
                  options={fields
                    .filter((field) => others.every((other) => other.field !== field))
                    .map((field) => ({ value: field, label: field }))}
                />
              </div>
              <div className="w-[136px] shrink-0">
                <FormSelect
                  ariaLabel={t('explore.runtime.partition_method')}
                  value={partition.scheme === 'prefix' ? 'prefix' : `hash:${partition.buckets}`}
                  onChange={(value) =>
                    setPartition(
                      value === 'prefix'
                        ? { ...partition, scheme: 'prefix' }
                        : { ...partition, scheme: 'hash', buckets: Number(value.slice('hash:'.length)) },
                    )
                  }
                  options={[
                    ...PARTITION_BUCKETS.map((buckets) => ({
                      value: `hash:${buckets}`,
                      label: t('explore.runtime.partition_methods.hash', { count: buckets }),
                      disabled: buckets > room,
                      disabledReason: tooMany,
                    })),
                    {
                      value: 'prefix',
                      label: t('explore.runtime.partition_methods.prefix', { count: PREFIX_SHARDS }),
                      disabled: PREFIX_SHARDS > room,
                      disabledReason: tooMany,
                    },
                  ]}
                />
              </div>
            </div>
          );
        }}
      />
      <span className="font-sans text-xs leading-relaxed text-tx-3">
        {t('explore.runtime.partition_field_hint')}
      </span>
    </div>
  );
}

/** 新字段按名字定索引：每行一个名字模式和它的索引类型，按次序第一条匹配的生效。 */
function IndexPatterns({
  draft,
  onChange,
}: {
  draft: StreamDraft;
  onChange: (patch: Partial<StreamDraft>) => void;
}) {
  const { t } = useTranslation('streams');
  const labelId = React.useId();

  return (
    <div role="group" aria-labelledby={labelId} className="flex flex-col gap-1.5">
      <span id={labelId} className={uiLabelClass}>
        {t('explore.runtime.index_pattern')}
      </span>
      <FieldArray<streamsApi.StreamIndexPattern>
        items={draft.indexPatterns}
        onChange={(indexPatterns) => onChange({ indexPatterns })}
        maxItems={MAX_INDEX_PATTERNS}
        newItem={() => ({ pattern: '', indexType: 'exact' })}
        addLabel={t('explore.runtime.index_pattern_add')}
        removeLabel={t('explore.runtime.index_pattern_remove')}
        reorderable
        reorderLabel={{
          up: t('explore.runtime.index_pattern_up'),
          down: t('explore.runtime.index_pattern_down'),
        }}
        rowClassName="items-center"
        renderItem={(pattern, _index, setPattern) => (
          <div className="flex gap-2">
            <div className="min-w-0 flex-1">
              <FormInput
                aria-label={t('explore.runtime.index_pattern_name')}
                value={pattern.pattern}
                placeholder="*_id"
                onChange={(event) => setPattern({ ...pattern, pattern: event.target.value })}
              />
            </div>
            <div className="w-[136px] shrink-0">
              <FormSelect
                ariaLabel={t('explore.schema.columns.index_type')}
                value={pattern.indexType}
                onChange={(value) =>
                  setPattern({ ...pattern, indexType: value as streamsApi.StreamIndexType })
                }
                options={INDEX_OPTIONS.map((option) => ({
                  value: option.value,
                  label: t(option.labelKey),
                }))}
              />
            </div>
          </div>
        )}
      />
      <span className="font-sans text-xs leading-relaxed text-tx-3">
        {t('explore.runtime.index_pattern_hint')}
      </span>
    </div>
  );
}

/** What a stream is for, whether and how far it is queried, and how logs and traces are stored. */
export function SettingsPanel({
  draft,
  mergesByDay,
  onChange,
}: {
  draft: StreamDraft;
  /** 按天合并只用于日志与链路。 */
  mergesByDay: boolean;
  onChange: (patch: Partial<StreamDraft>) => void;
}) {
  const { t } = useTranslation('streams');

  return (
    // 日志与链路多一张存储行为的卡片：四张排成两列，三张排成一行。
    <div className={mergesByDay ? 'grid gap-4 xl:grid-cols-2' : 'grid gap-4 xl:grid-cols-3'}>
      <StreamSettingsSection
        title={t('explore.runtime.basic')}
        description={t('explore.runtime.basic_hint')}
      >
        <FormField label={t('explore.runtime.description_label')}>
          <FormTextarea
            value={draft.description}
            onChange={(event) => onChange({ description: event.target.value })}
            rows={5}
            placeholder={t('explore.runtime.description_placeholder')}
          />
        </FormField>
      </StreamSettingsSection>

      <StreamSettingsSection
        title={t('explore.runtime.query_capabilities')}
        description={t('explore.runtime.query_capabilities_hint')}
      >
        <StreamToggleRow
          title={t('explore.runtime.queryable')}
          hint={t('explore.runtime.queryable_hint')}
          checked={draft.queryable}
          onChange={(queryable) => onChange({ queryable })}
        />
        <FormField
          label={t('explore.runtime.max_query_range')}
          hint={t('explore.runtime.max_query_range_hint')}
        >
          <FormInput
            type="number"
            min={0}
            value={draft.maxQueryRange}
            onChange={(event) => onChange({ maxQueryRange: event.target.value })}
          />
        </FormField>
      </StreamSettingsSection>

      {mergesByDay && (
        <StreamSettingsSection
          title={t('explore.runtime.storage_behavior')}
          description={t('explore.runtime.storage_behavior_hint')}
        >
          <StreamToggleRow
            title={t('explore.runtime.daily_merge')}
            hint={t('explore.runtime.daily_merge_hint')}
            checked={draft.dailyMerge}
            onChange={(dailyMerge) => onChange({ dailyMerge })}
          />
          <PartitionFields draft={draft} onChange={onChange} />
        </StreamSettingsSection>
      )}

      <StreamSettingsSection
        title={t('explore.runtime.field_indexes')}
        description={t('explore.runtime.field_indexes_hint')}
      >
        <IndexPatterns draft={draft} onChange={onChange} />
      </StreamSettingsSection>
    </div>
  );
}
