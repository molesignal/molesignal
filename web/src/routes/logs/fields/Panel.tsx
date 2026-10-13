import { RefreshCw, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { CollapsibleSidePanel, SidePanelSection } from '@/shell/CollapsibleSidePanel';
import { cn } from '@/shell/lib/cn';
import { FIELD_LIST_GROUND_CLASS } from '@/shell/query/fields/Group';
import type { HistogramRange } from '@/shell/query/histogram/model';
import { QueryState } from '@/shell/query/State';
import { TooltipProvider } from '@/shell/ui/tooltip';

import type { LogFieldDef } from '../fieldQueryModel';
import type { CountedLogQuery } from '../histogramCounts';
import type { LogQueryMode } from '../queryState';
import { LogFieldAnalysis } from './Analysis';
import { LogFieldList } from './List';

interface LogFieldPanelProps {
  collapsed: boolean;
  onCollapsedChange: (collapsed: boolean) => void;
  /** Every field of the stream and of the results. */
  fields: LogFieldDef[];
  /** The fields on display, in the order of the columns. */
  visibleFields: string[];
  queryMode: LogQueryMode;
  /** The loaded records: a field's count, its values and the fallback of its analysis are of these. */
  records: Record<string, unknown>[];
  /** When each loaded record was written, in microseconds, in their order. */
  moments: ReadonlyArray<number | null>;
  /** The first results are on their way. */
  loading: boolean;
  /** Whether a stream is picked: what an empty list says depends on it. */
  hasStream: boolean;
  canRefresh: boolean;
  onRefresh: () => void;
  expandedField: string | null;
  onExpandedFieldChange: (field: string | null) => void;
  onToggleVisibility: (field: string) => void;
  onInsertFilter: (field: string) => void;
  onInsertValueFilter: (field: string, value: unknown, mode: 'include' | 'exclude') => void;
  /** The field query the records came from and the range it ran over: an analysis reads all it matches. */
  counted: CountedLogQuery | null;
  range: HistogramRange | null;
  /** The fields masked for this reader, lower-cased; `null` until that is known. */
  maskedFields: ReadonlySet<string> | null;
}

/**
 * The field list of the log explorer: the fields of the stream and of the
 * results in three parts (on display, indexed, the rest), a search over their
 * names, and beside it the analysis of the field one was asked for.
 */
export function LogFieldPanel({
  collapsed,
  onCollapsedChange,
  fields,
  visibleFields,
  queryMode,
  records,
  moments,
  loading,
  hasStream,
  canRefresh,
  onRefresh,
  expandedField,
  onExpandedFieldChange,
  onToggleVisibility,
  onInsertFilter,
  onInsertValueFilter,
  counted,
  range,
  maskedFields,
}: LogFieldPanelProps) {
  const { t } = useTranslation('logs');
  const [fieldFilter, setFieldFilter] = React.useState('');
  const [analysedField, setAnalysedField] = React.useState<string | null>(null);
  const searching = fieldFilter.trim() !== '';
  const filteredFields = React.useMemo(() => {
    const needle = fieldFilter.trim().toLowerCase();

    return fields.filter((field) => field.name.toLowerCase().includes(needle));
  }, [fieldFilter, fields]);
  const sections = React.useMemo(() => {
    // What is not on display is divided by whether the stream keeps an index on it: a filter on an
    // indexed field is quick, on another it reads every row.
    const hidden = filteredFields.filter((field) => !visibleFields.includes(field.name));

    return [
      // The fields on display are the columns of the list, in their order: they stay a plain list.
      {
        key: 'visible_fields',
        fields: visibleFields
          .map((name) => filteredFields.find((field) => field.name === name))
          .filter((field): field is LogFieldDef => field !== undefined),
        visible: true,
        grouped: false,
      },
      // The rest are looked through: the fields of `http` stand under `http`.
      { key: 'indexed_fields', fields: hidden.filter((field) => field.indexed === true), visible: false, grouped: true },
      { key: 'other_fields', fields: hidden.filter((field) => field.indexed !== true), visible: false, grouped: true },
    ] as const;
  }, [filteredFields, visibleFields]);
  // A query that failed is said once, where its results would be. The fields of the stream are
  // still its fields: they stay in the list, which is where the query is put right from.
  const listState: 'loading' | 'empty' | null =
    fields.length > 0 ? null : loading ? 'loading' : 'empty';
  // There is something to analyse once there are results, and for as long as the field is listed.
  const analysable = records.length > 0;
  const analysed = analysable ? fields.find((field) => field.name === analysedField) : undefined;

  return (
    <CollapsibleSidePanel
      title={t('explore.left_panel.title')}
      collapsed={collapsed}
      onCollapsedChange={onCollapsedChange}
      variant="utility"
      widthClassName="w-[240px]"
      resizable
      defaultWidth={240}
      resizeLabel={t('explore.left_panel.resize')}
      bodyClassName="flex flex-col"
      className="rounded-md border-r-0 bg-[var(--functional-surface)]"
      collapseLabel={t('explore.left_panel.collapse')}
      expandLabel={t('explore.left_panel.expand')}
      flyout={
        analysed && (
          <LogFieldAnalysis
            // Another field is another analysis: nothing of the one before is kept on screen.
            key={analysed.name}
            field={analysed}
            counted={counted}
            range={range}
            records={records}
            moments={moments}
            masked={maskedFields?.has(analysed.name.toLowerCase()) ?? false}
            onClose={() => setAnalysedField(null)}
          />
        )
      }
      footer={
        <div className="flex h-11 items-center justify-between px-2 font-sans text-xs font-strong text-tx-2">
          <span className="min-w-0 truncate">
            {t('explore.left_panel.fields_summary', {
              shown: filteredFields.length.toLocaleString(),
              total: fields.length.toLocaleString(),
              visible: visibleFields.length.toLocaleString(),
            })}
          </span>
          <button
            aria-label={t('explore.left_panel.refresh_fields_aria')}
            className="grid h-8 w-8 place-items-center rounded-md hover:bg-bg-3 disabled:cursor-not-allowed disabled:opacity-50"
            disabled={!canRefresh}
            onClick={onRefresh}
          >
            <RefreshCw className="h-2.5 w-2.5" />
          </button>
        </div>
      }
    >
      <div className="px-2 pb-2">
        <div className="flex h-8 items-center gap-2 rounded-md border-0 bg-[var(--control-surface)] px-2.5 font-sans text-xs">
          <Search className="h-3.5 w-3.5 text-tx-3" />
          <input
            value={fieldFilter}
            onChange={(event) => setFieldFilter(event.target.value)}
            placeholder={t('explore.left_panel.field_search_placeholder')}
            aria-label={t('explore.left_panel.filter_fields_aria')}
            className="flex-1 bg-transparent text-tx-0 placeholder:text-tx-3 focus:outline-none"
          />
        </div>
      </div>
      <div className={cn('mx-2 mb-1 flex-1 overflow-auto', FIELD_LIST_GROUND_CLASS)}>
        {listState ? (
          <QueryState
            state={listState}
            loadingLabel={t('explore.left_panel.loading_fields')}
            emptyLabel={
              hasStream
                ? t('explore.left_panel.no_fields_from_query')
                : t('explore.left_panel.pick_stream_prompt')
            }
            className="h-full min-h-32 px-4 text-center"
          />
        ) : filteredFields.length === 0 ? (
          <QueryState
            state="empty"
            emptyLabel={t('explore.left_panel.no_matching_fields')}
            className="h-full min-h-32 px-4 text-center"
          />
        ) : (
          <TooltipProvider delayDuration={250}>
            {sections.map(
              (section) =>
                // A part that holds nothing has no heading, but for the indexed fields: that a stream
                // keeps none in an index is worth saying, where no search has cut the list.
                (section.fields.length > 0 || (section.key === 'indexed_fields' && !searching)) && (
                  <SidePanelSection
                    key={section.key}
                    title={t(`explore.left_panel.${section.key}`)}
                    count={section.fields.length}
                  >
                    {section.fields.length === 0 && (
                      <p data-field-list-empty className="px-1.5 pb-1 font-sans text-xs text-tx-2">
                        {t('explore.left_panel.no_indexed_fields')}
                      </p>
                    )}
                    <LogFieldList
                      fields={section.fields}
                      grouped={section.grouped}
                      searching={searching}
                      visible={section.visible}
                      queryMode={queryMode}
                      records={records}
                      expandedField={expandedField}
                      onExpandedFieldChange={onExpandedFieldChange}
                      analysedField={analysed?.name ?? null}
                      onAnalysedFieldChange={analysable ? setAnalysedField : undefined}
                      onToggleVisibility={onToggleVisibility}
                      onInsertFilter={onInsertFilter}
                      onInsertValueFilter={onInsertValueFilter}
                    />
                  </SidePanelSection>
                ),
            )}
          </TooltipProvider>
        )}
      </div>
    </CollapsibleSidePanel>
  );
}
