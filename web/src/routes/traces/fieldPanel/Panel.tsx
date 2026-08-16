import { ChevronRight, Eye, EyeOff, Plus, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { CollapsibleSidePanel, SidePanelSection } from '@/shell/CollapsibleSidePanel';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

import {
  groupTraceFields,
  isTraceFieldQueryable,
  type TraceFieldDef,
  type TraceFieldName,
  type TraceQueryMode,
} from '../fieldQueryModel';
import {
  isTraceFieldDisplayable,
  TRACE_TYPE_COLOR,
  TRACE_TYPE_GLYPH,
  traceFieldCount,
  traceFieldTopValues,
  type TraceFieldRecord,
} from './model';

interface TraceFieldPanelProps {
  traces: TraceFieldRecord[];
  fields: TraceFieldDef[];
  queryMode: TraceQueryMode;
  visibleFields: TraceFieldName[];
  fieldFilter: string;
  collapsed: boolean;
  onFieldFilterChange: (value: string) => void;
  onCollapsedChange: (collapsed: boolean) => void;
  onToggleField: (field: TraceFieldName) => void;
  onInsertField: (field: TraceFieldDef) => void;
}

export function TraceFieldPanel({
  traces,
  fields,
  queryMode,
  visibleFields,
  fieldFilter,
  collapsed,
  onFieldFilterChange,
  onCollapsedChange,
  onToggleField,
  onInsertField,
}: TraceFieldPanelProps) {
  const { t } = useTranslation('traces');
  const needle = fieldFilter.trim().toLowerCase();
  const filtered = React.useMemo(
    () => fields.filter((field) => field.name.toLowerCase().includes(needle)),
    [fields, needle],
  );
  const { core, groups } = React.useMemo(() => groupTraceFields(filtered), [filtered]);
  const attributes = React.useMemo(
    () => groups.flatMap((group) => group.fields),
    [groups],
  );
  const [expandedField, setExpandedField] = React.useState<TraceFieldName | null>(null);

  const renderRow = (field: TraceFieldDef) => (
    <TraceFieldRow
      key={field.name}
      field={field}
      queryMode={queryMode}
      traces={traces}
      visible={visibleFields.includes(field.name)}
      expanded={expandedField === field.name}
      onExpandedChange={(open) => setExpandedField(open ? field.name : null)}
      onToggleField={onToggleField}
      onInsertField={onInsertField}
    />
  );

  return (
    <CollapsibleSidePanel
      title={t('explore.labels.title')}
      collapsed={collapsed}
      onCollapsedChange={onCollapsedChange}
      variant="utility"
      widthClassName="w-[240px]"
      resizable
      defaultWidth={240}
      resizeLabel={t('explore.labels.resize')}
      bodyClassName="flex flex-col"
      collapseLabel={t('explore.labels.collapse')}
      expandLabel={t('explore.labels.expand')}
      footer={
        <div className="flex h-11 items-center truncate border-t border-bd-0 px-3 font-sans text-xs text-tx-3">
          {t('explore.labels.fields_summary', {
            shown: filtered.length,
            total: fields.length,
          })}
        </div>
      }
    >
      <TooltipProvider>
        <div className="px-2 pb-2">
          <div className="flex h-8 items-center gap-2 rounded-md border border-bd-1 bg-bg-1 px-2.5 font-sans text-xs">
            <Search className="h-3.5 w-3.5 text-tx-3" />
            <input
              value={fieldFilter}
              onChange={(event) => onFieldFilterChange(event.target.value)}
              placeholder={t('explore.labels.filter_placeholder')}
              aria-label={t('explore.labels.filter_aria')}
              className="min-w-0 flex-1 bg-transparent text-tx-0 placeholder:text-tx-3 focus:bg-bg-2"
            />
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-auto px-1">
          {core.length > 0 && (
            <SidePanelSection title={t('explore.labels.core_fields')} count={core.length}>
              {core.map(renderRow)}
            </SidePanelSection>
          )}
          {attributes.length > 0 && (
            <SidePanelSection title={t('explore.labels.attributes')} count={attributes.length}>
              {attributes.map(renderRow)}
            </SidePanelSection>
          )}
          {filtered.length === 0 && (
            <div className="px-2 py-4 text-center font-sans text-xs text-tx-3">
              {t('explore.results.no_match')}
            </div>
          )}
        </div>
      </TooltipProvider>
    </CollapsibleSidePanel>
  );
}

interface TraceFieldRowProps {
  field: TraceFieldDef;
  queryMode: TraceQueryMode;
  traces: TraceFieldRecord[];
  visible: boolean;
  expanded: boolean;
  onExpandedChange: (open: boolean) => void;
  onToggleField: (field: TraceFieldName) => void;
  onInsertField: (field: TraceFieldDef) => void;
}

function TraceFieldRow({
  field,
  queryMode,
  traces,
  visible,
  expanded,
  onExpandedChange,
  onToggleField,
  onInsertField,
}: TraceFieldRowProps) {
  const { t } = useTranslation('traces');
  const displayable = isTraceFieldDisplayable(field.name);
  const queryable = isTraceFieldQueryable(field, queryMode);
  const count = displayable ? traceFieldCount(traces, field.name) : 0;
  const topValues = displayable ? traceFieldTopValues(traces, field.name) : [];
  const toggleLabel = visible
    ? t('explore.labels.hide_field_aria', { name: field.name })
    : t('explore.labels.show_field_aria', { name: field.name });
  const unavailableReason = field.dataType === 'json'
    ? t('explore.labels.json_query_required', { name: field.name })
    : t('explore.labels.direct_query_unavailable', { name: field.name });
  const addLabel = queryable
    ? t('explore.labels.add_field_aria', { name: field.name })
    : unavailableReason;

  return (
    <div data-trace-field={field.name}>
      <div className="group flex min-h-9 items-center gap-1 px-1.5 font-sans text-xs font-strong hover:bg-bg-3">
        <button
          type="button"
          onClick={() => onExpandedChange(!expanded)}
          disabled={!displayable}
          className="flex min-w-0 flex-1 items-center gap-2 rounded px-1 py-1.5 text-left focus:bg-bg-3 disabled:cursor-default"
          aria-expanded={displayable ? expanded : undefined}
        >
          <ChevronRight
            className={`h-3 w-3 shrink-0 text-tx-3 transition-transform ${displayable ? '' : 'invisible'} ${expanded ? 'rotate-90' : ''}`}
          />
          <span className={`w-4 shrink-0 text-center text-xs font-bold ${TRACE_TYPE_COLOR[field.dataType]}`}>
            {TRACE_TYPE_GLYPH[field.dataType]}
          </span>
          <span className="min-w-0 flex-1 truncate text-tx-0">{field.name}</span>
          <span className="type-micro shrink-0 font-mono font-normal text-tx-2">
            {displayable ? count : ''}
          </span>
        </button>
        {displayable ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() => onToggleField(field.name)}
                className="grid h-7 w-7 shrink-0 place-items-center rounded text-tx-3 opacity-0 hover:bg-bg-4 hover:text-tx-0 focus:opacity-100 group-hover:opacity-100"
                aria-label={toggleLabel}
              >
                {visible ? <Eye className="h-3 w-3" /> : <EyeOff className="h-3 w-3" />}
              </button>
            </TooltipTrigger>
            <TooltipContent>{toggleLabel}</TooltipContent>
          </Tooltip>
        ) : null}
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              onClick={() => onInsertField(field)}
              disabled={!queryable}
              className="grid h-7 w-7 shrink-0 place-items-center rounded text-tx-3 opacity-0 hover:bg-bg-4 hover:text-blue-soft focus:opacity-100 disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-tx-3 group-hover:opacity-100"
              aria-label={addLabel}
            >
              <Plus className="h-3 w-3" />
            </button>
          </TooltipTrigger>
          <TooltipContent>{addLabel}</TooltipContent>
        </Tooltip>
      </div>
      {expanded && (
        <div data-trace-field-values={field.name} className="bg-bg-1 px-2 py-1.5">
          {topValues.length === 0 ? (
            <div className="px-1 py-2 font-sans text-xs text-tx-2">
              {t('explore.labels.no_top_values')}
            </div>
          ) : (
            <div className="space-y-0.5">
              {topValues.map((topValue) => (
                <div
                  key={topValue.label}
                  className="flex min-h-8 items-center gap-2 rounded px-1 hover:bg-bg-3"
                >
                  <span
                    className="type-micro min-w-0 flex-1 truncate font-mono text-tx-1"
                    title={topValue.label}
                  >
                    {topValue.label}
                  </span>
                  <span className="type-micro w-7 shrink-0 text-right font-mono text-tx-2">
                    {topValue.count}
                  </span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
