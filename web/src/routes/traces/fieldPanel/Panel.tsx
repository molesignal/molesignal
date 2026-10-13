import { ChevronDown, Eye, EyeOff, Plus, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { CollapsibleSidePanel, SidePanelSection } from '@/shell/CollapsibleSidePanel';
import { cn } from '@/shell/lib/cn';
import { FieldAnalysisTrigger } from '@/shell/query/fieldAnalysis/Trigger';
import {
  FIELD_CARD_CLASS,
  FIELD_CARD_HEAD_CLASS,
  FIELD_LIST_GROUND_CLASS,
  FIELD_NEST_CLASS,
  FIELD_NESTED_ROW_CLASS,
  FieldGroup,
} from '@/shell/query/fields/Group';
import { fieldTree, fieldTreeSize, type FieldTreeLeaf, type FieldTreeNode } from '@/shell/query/fields/tree';
import { FieldTypeChip } from '@/shell/query/fields/TypeChip';
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
import type { TraceValueOperator } from '../valueFilter';
import { TraceFieldAnalysis } from './Analysis';
import {
  isTraceFieldExpandable,
  isTraceListField,
  TRACE_TYPE_GLYPH,
  TRACE_TYPE_TONE,
  type TraceFieldRecord,
} from './model';
import { TraceFieldValuesBlock } from './Values';
import type { TraceFieldValueSource } from './valuesQuery';

interface TraceFieldPanelProps {
  /** The traces on the page: a field's values are read from these, and counted in them. */
  traces: TraceFieldRecord[];
  fields: TraceFieldDef[];
  /** Where the values of a span's field are read from. */
  source: TraceFieldValueSource;
  /** The language a filter would be written in. */
  filterMode: TraceQueryMode;
  fieldFilter: string;
  collapsed: boolean;
  onFieldFilterChange: (value: string) => void;
  onCollapsedChange: (collapsed: boolean) => void;
  /** The fields shown beside a trace that is picked, where the view has such a place. */
  visibleFields?: TraceFieldName[];
  onToggleField?: (field: TraceFieldName) => void;
  onInsertField: (field: TraceFieldDef) => void;
  isValueFiltered: (field: TraceFieldDef, value: string, op: TraceValueOperator) => boolean;
  onFilterValue: (field: TraceFieldDef, value: string, op: TraceValueOperator) => void;
  className?: string;
}

export function TraceFieldPanel({
  traces,
  fields,
  source,
  filterMode,
  fieldFilter,
  collapsed,
  onFieldFilterChange,
  onCollapsedChange,
  visibleFields,
  onToggleField,
  onInsertField,
  isValueFiltered,
  onFilterValue,
  className,
}: TraceFieldPanelProps) {
  const { t } = useTranslation('traces');
  const { t: tCommon } = useTranslation('common');
  const needle = fieldFilter.trim().toLowerCase();
  const filtered = React.useMemo(
    () => fields.filter((field) => field.name.toLowerCase().includes(needle)),
    [fields, needle],
  );
  // The list is divided by what a field is to the reader: on display beside a trace, kept in an
  // index (a filter on it is quick), or neither. Within a part, the fields a trace is told by come
  // first under their own names; the attributes after them are looked through, so the fields of
  // `http` stand under `http`.
  const sections = React.useMemo(() => {
    const { core, groups } = groupTraceFields(filtered);
    const attributes = groups.flatMap((group) => group.fields);
    const shown = new Set(onToggleField ? visibleFields ?? [] : []);
    const part = (keep: (field: TraceFieldDef) => boolean): FieldTreeNode<TraceFieldDef>[] => [
      ...core.filter(keep).map((field) => ({ kind: 'field' as const, field, label: field.name })),
      ...fieldTree(attributes.filter(keep)),
    ];

    return [
      // What is on display is a plain list: a field keeps its whole name there.
      {
        key: 'displayed_fields',
        nodes: [...core, ...attributes]
          .filter((field) => shown.has(field.name))
          .map((field) => ({ kind: 'field' as const, field, label: field.name })),
      },
      { key: 'indexed_fields', nodes: part((field) => !shown.has(field.name) && field.indexed === true) },
      { key: 'other_fields', nodes: part((field) => !shown.has(field.name) && field.indexed !== true) },
    ] as const;
  }, [filtered, onToggleField, visibleFields]);
  const [expandedField, setExpandedField] = React.useState<TraceFieldName | null>(null);
  const [analysedField, setAnalysedField] = React.useState<TraceFieldName | null>(null);
  const [openGroups, setOpenGroups] = React.useState<ReadonlySet<string>>(() => new Set());
  // A SQL statement may list a trace on several rows; the field list counts traces.
  const loaded = React.useMemo(() => {
    const seen = new Set<string>();
    return traces.filter((trace) => trace.id !== '' && !seen.has(trace.id) && Boolean(seen.add(trace.id)));
  }, [traces]);
  const canReadSpans = Boolean(source.orgId && source.stream);
  // There is something to analyse once traces are listed, and for as long as the field is.
  const analysable = (field: TraceFieldDef) => loaded.length > 0 && isTraceFieldExpandable(field, canReadSpans);
  const analysed = fields.find((field) => field.name === analysedField && analysable(field));

  const renderRow = ({ field, label }: FieldTreeLeaf<TraceFieldDef>, nested = false) => {
    const expanded = expandedField === field.name;
    // The eye picks what is shown beside a picked trace, which only has what its row carries.
    const toggle = onToggleField && isTraceListField(field.name)
      ? { visible: visibleFields?.includes(field.name) ?? false, onToggle: () => onToggleField(field.name) }
      : undefined;

    return (
      <TraceFieldRow
        key={field.name}
        field={field}
        label={label}
        nested={nested}
        queryable={isTraceFieldQueryable(field, filterMode)}
        expandable={isTraceFieldExpandable(field, canReadSpans)}
        expanded={expanded}
        onExpandedChange={(open) => setExpandedField(open ? field.name : null)}
        onInsertField={onInsertField}
        {...(toggle ? { toggle } : {})}
        {...(analysable(field)
          ? {
              analysis: {
                open: analysed?.name === field.name,
                onToggle: () => setAnalysedField(analysed?.name === field.name ? null : field.name),
              },
            }
          : {})}
      >
        {expanded && (
          <TraceFieldValuesBlock
            field={field}
            traces={loaded}
            source={source}
            filterMode={filterMode}
            isValueFiltered={isValueFiltered}
            onFilterValue={onFilterValue}
          />
        )}
      </TraceFieldRow>
    );
  };

  return (
    <CollapsibleSidePanel
      title={t('explore.labels.title')}
      collapsed={collapsed}
      onCollapsedChange={onCollapsedChange}
      variant="utility"
      widthClassName="w-[260px]"
      resizable
      defaultWidth={260}
      resizeLabel={t('explore.labels.resize')}
      bodyClassName="flex flex-col"
      className={cn('border-r-0', className)}
      collapseLabel={t('explore.labels.collapse')}
      expandLabel={t('explore.labels.expand')}
      flyout={
        analysed && (
          <TraceFieldAnalysis
            // Another field is another analysis: nothing of the one before is kept on screen.
            key={analysed.name}
            field={analysed}
            traces={loaded}
            source={source}
            onClose={() => setAnalysedField(null)}
          />
        )
      }
      footer={
        <div className="flex h-11 items-center truncate px-3 font-sans text-xs text-tx-3">
          {t('explore.labels.fields_summary', {
            shown: filtered.length,
            total: fields.length,
          })}
        </div>
      }
    >
      <TooltipProvider>
        <div className="px-2 pb-2">
          <div className="flex h-8 items-center gap-2 rounded-md border-0 bg-[var(--control-surface)] px-2.5 font-sans text-xs">
            <Search className="h-3.5 w-3.5 text-tx-3" />
            <input
              value={fieldFilter}
              onChange={(event) => onFieldFilterChange(event.target.value)}
              placeholder={t('explore.labels.filter_placeholder')}
              aria-label={t('explore.labels.filter_aria')}
              className="min-w-0 flex-1 bg-transparent text-tx-0 placeholder:text-tx-3 focus:bg-transparent"
            />
          </div>
        </div>
        <div className={cn('mx-2 mb-1 min-h-0 flex-1 overflow-auto', FIELD_LIST_GROUND_CLASS)}>
          {sections.map(
            (section) =>
              // A part that holds nothing has no heading, but for the indexed fields: that a stream
              // keeps none in an index is worth saying, where no search has cut the list.
              (section.nodes.length > 0 || (section.key === 'indexed_fields' && needle === '')) && (
                <SidePanelSection
                  key={section.key}
                  title={t(`explore.labels.${section.key}`)}
                  count={fieldTreeSize(section.nodes)}
                >
                  {section.nodes.length === 0 && (
                    <p data-field-list-empty className="px-1.5 pb-1 font-sans text-xs text-tx-2">
                      {t('explore.labels.no_indexed_fields')}
                    </p>
                  )}
                  {section.nodes.map((node) => {
                    if (node.kind === 'field') return renderRow(node);

                    const holdsOpenField = node.children.some((child) => child.field.name === expandedField);

                    return (
                      <FieldGroup
                        key={`group:${node.name}`}
                        name={node.name}
                        count={node.children.length}
                        // A search has cut the list to what matches; a field open on its values keeps its group open.
                        open={needle !== '' || openGroups.has(node.name) || holdsOpenField}
                        onOpenChange={(open) => {
                          setOpenGroups((current) => {
                            const next = new Set(current);
                            if (open) next.add(node.name);
                            else next.delete(node.name);

                            return next;
                          });
                          if (!open && holdsOpenField) setExpandedField(null);
                        }}
                        label={tCommon('field_list.group_aria', { name: node.name, count: node.children.length })}
                      >
                        {node.children.map((child) => renderRow(child, true))}
                      </FieldGroup>
                    );
                  })}
                </SidePanelSection>
              ),
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
  /** The name the row is listed under: the field's own, or the rest of it inside a group. */
  label: string;
  /** The row is one of a group's, on the group's ground. */
  nested: boolean;
  queryable: boolean;
  expandable: boolean;
  expanded: boolean;
  onExpandedChange: (open: boolean) => void;
  onInsertField: (field: TraceFieldDef) => void;
  toggle?: { visible: boolean; onToggle: () => void };
  /** What the field holds across the listed traces can be opened from the row. */
  analysis?: { open: boolean; onToggle: () => void };
  /** The field's values, when it is open. */
  children: React.ReactNode;
}

const ROW_ACTION = 'grid h-7 w-7 shrink-0 place-items-center rounded text-tx-3 hover:bg-bg-4 focus-visible:bg-bg-4';
/**
 * What can be asked of a field stands at the end of its row while the pointer is
 * on the row or the focus is in it, and takes no room otherwise: a name is cut
 * short for buttons that are there, not for the place where they would be. A row
 * that was clicked holds the focus, so Tab goes on to its buttons from there.
 */
const ROW_ACTIONS = 'hidden shrink-0 items-center group-hover:flex group-focus-within:flex';
/** The same for an action that stands outside that group: gone until the row is pointed at or holds the focus. */
const ROW_ACTION_AT_REST = 'hidden group-hover:grid group-focus-within:grid';

function TraceFieldRow({
  field,
  label,
  nested,
  queryable,
  expandable,
  expanded,
  onExpandedChange,
  onInsertField,
  toggle,
  analysis,
  children,
}: TraceFieldRowProps) {
  const { t } = useTranslation('traces');
  const valuesId = React.useId();
  const toggleLabel = toggle?.visible
    ? t('explore.labels.hide_field_aria', { name: field.name })
    : t('explore.labels.show_field_aria', { name: field.name });
  const unavailableReason = field.dataType === 'json'
    ? t('explore.labels.json_query_required', { name: field.name })
    : t('explore.labels.direct_query_unavailable', { name: field.name });
  const addLabel = queryable
    ? t('explore.labels.add_field_aria', { name: field.name })
    : unavailableReason;

  return (
    <div
      data-trace-field={field.name}
      data-field-row
      // A field of the list is a card; a field under a group is a row on a ground of its own inside the group's card.
      className={nested ? FIELD_NESTED_ROW_CLASS : FIELD_CARD_CLASS}
    >
      {/* The head carries its own state, from edge to edge: under the pointer or the keyboard it is
          lit whole. The button inside has no fill of its own, which would light only the part of
          the row it covers. */}
      <div
        data-trace-field-row
        className={cn(
          'group flex items-center gap-1 pr-1 font-sans text-xs',
          nested ? 'min-h-8 rounded-[5px] pl-1.5 hover:bg-bg-3 has-[:focus-visible]:bg-bg-4' : cn('min-h-9 pl-2', FIELD_CARD_HEAD_CLASS),
        )}
      >
        <button
          type="button"
          onClick={() => onExpandedChange(!expanded)}
          disabled={!expandable}
          className="flex min-w-0 flex-1 items-center gap-2 py-1.5 text-left focus-visible:bg-transparent disabled:cursor-default"
          aria-expanded={expandable ? expanded : undefined}
          aria-controls={expanded ? valuesId : undefined}
        >
          <FieldTypeChip glyph={TRACE_TYPE_GLYPH[field.dataType]} tone={TRACE_TYPE_TONE[field.dataType]} nested={nested} />
          <span className="min-w-0 truncate font-body text-tx-0" title={field.name}>
            {label}
          </span>
          {/* The row opens on its values: the arrow shows where the pointer or the keyboard is, and while it is open. */}
          {expandable && (
            <ChevronDown
              aria-hidden="true"
              className={cn(
                'h-3 w-3 shrink-0 text-tx-3 transition-transform',
                expanded ? 'rotate-180' : 'opacity-0 group-hover:opacity-100 group-focus-within:opacity-100',
              )}
            />
          )}
        </button>
        {/* One button whether its panel is open or not, so the keyboard can be given back to it. It
            shows with the row's other actions, and stays in view for as long as its panel is open. */}
        {analysis && (
          <FieldAnalysisTrigger
            name={field.name}
            pressed={analysis.open}
            onPress={analysis.onToggle}
            className={cn(ROW_ACTION, !analysis.open && ROW_ACTION_AT_REST)}
          />
        )}
        <div className={ROW_ACTIONS}>
          {toggle ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  onClick={toggle.onToggle}
                  className={cn(ROW_ACTION, 'hover:text-tx-0 focus-visible:text-tx-0')}
                  aria-label={toggleLabel}
                  aria-pressed={toggle.visible}
                >
                  {toggle.visible ? <Eye className="h-3 w-3" /> : <EyeOff className="h-3 w-3" />}
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
                className={cn(
                  ROW_ACTION,
                  'hover:text-[var(--blue-ink)] focus-visible:text-[var(--blue-ink)] disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-tx-3',
                )}
                aria-label={addLabel}
              >
                <Plus className="h-3 w-3" />
              </button>
            </TooltipTrigger>
            <TooltipContent>{addLabel}</TooltipContent>
          </Tooltip>
        </div>
      </div>
      {expanded && (
        <div
          id={valuesId}
          data-trace-field-values={field.name}
          // What a field opens on is inside it, set in, on a ground of its own; a field under a
          // group stands on that ground already and takes the surface of the card instead.
          className={
            nested ? 'mx-[3px] mb-[3px] rounded-[4px] bg-[var(--functional-surface)] p-[4px]' : FIELD_NEST_CLASS
          }
        >
          {children}
        </div>
      )}
    </div>
  );
}
