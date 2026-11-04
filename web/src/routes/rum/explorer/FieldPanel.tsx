import { ChevronDown, Eye, EyeOff, Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumField, RumFieldType } from '@/api/rum';
import { CollapsibleSidePanel, SidePanelSearch, SidePanelSection } from '@/shell/CollapsibleSidePanel';
import { cn } from '@/shell/lib/cn';
import {
  FIELD_FLAT_HEAD_CLASS,
  FIELD_FLAT_LINE_CLASS,
  FIELD_FLAT_NEST_CLASS,
  FIELD_FLAT_ROW_CLASS,
  FieldGroup,
} from '@/shell/query/fields/Group';
import { fieldTree, fieldTreeSize, type FieldTreeLeaf, type FieldTreeNode } from '@/shell/query/fields/tree';
import { type FieldTypeTone, FieldTypeChip } from '@/shell/query/fields/TypeChip';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shell/ui/tooltip';

import { isRumFieldQueryable } from './fieldQuery';
import { RumFieldValuesBlock, type RumValueSource } from './FieldValues';
import type { RumValueOperator } from './valueFilter';

const TYPE_GLYPH: Record<RumFieldType, string> = {
  bool: 'B',
  int64: '#',
  float64: '#',
  utf8: 'T',
  timestamp: 'T',
  json: '{}',
};

const TYPE_TONE: Record<RumFieldType, FieldTypeTone> = {
  bool: 'blue',
  int64: 'orange',
  float64: 'orange',
  utf8: 'green',
  timestamp: 'green',
  json: 'purple',
};

interface RumFieldPanelProps {
  fields: readonly RumField[];
  /** Where the values of a field are read from. */
  source: RumValueSource;
  language: string;
  /** The columns of the table: the eye puts a field there. */
  visibleColumns: readonly string[];
  onToggleColumn: (name: string) => void;
  fieldFilter: string;
  collapsed: boolean;
  onFieldFilterChange: (value: string) => void;
  onCollapsedChange: (collapsed: boolean) => void;
  onInsertField: (field: RumField) => void;
  isValueFiltered: (field: RumField, value: string, op: RumValueOperator) => boolean;
  onFilterValue: (field: RumField, value: string, op: RumValueOperator) => void;
  className?: string;
}

/**
 * The fields of the rows on the page, divided by what a field is to the reader: on display as a
 * column of the table, kept in an index (a filter on it is quick), or neither. A field opens on
 * the values it has across the rows the query matches.
 */
export function RumFieldPanel({
  fields,
  source,
  language,
  visibleColumns,
  onToggleColumn,
  fieldFilter,
  collapsed,
  onFieldFilterChange,
  onCollapsedChange,
  onInsertField,
  isValueFiltered,
  onFilterValue,
  className,
}: RumFieldPanelProps) {
  const { t } = useTranslation('rum');
  const { t: tCommon } = useTranslation('common');
  const needle = fieldFilter.trim().toLowerCase();
  const filtered = React.useMemo(
    () => fields.filter((field) => field.name.toLowerCase().includes(needle)),
    [fields, needle],
  );
  const [expandedField, setExpandedField] = React.useState<string | null>(null);
  const [openGroups, setOpenGroups] = React.useState<ReadonlySet<string>>(() => new Set());

  const sections = React.useMemo(() => {
    const shown = new Set(visibleColumns);
    const part = (keep: (field: RumField) => boolean): FieldTreeNode<RumField>[] => fieldTree(filtered.filter(keep));

    return [
      // What is on display is a plain list, in the order of the columns: a field keeps its whole name there.
      {
        key: 'displayed_fields',
        nodes: visibleColumns
          .map((name) => filtered.find((field) => field.name === name))
          .flatMap((field) => (field ? [{ kind: 'field' as const, field, label: field.name }] : [])),
      },
      { key: 'indexed_fields', nodes: part((field) => !shown.has(field.name) && field.indexed) },
      { key: 'other_fields', nodes: part((field) => !shown.has(field.name) && !field.indexed) },
    ] as const;
  }, [filtered, visibleColumns]);

  const renderRow = ({ field, label }: FieldTreeLeaf<RumField>, nested = false) => {
    const expanded = expandedField === field.name;

    return (
      <RumFieldRow
        key={field.name}
        field={field}
        label={label}
        nested={nested}
        expandable={field.facetable}
        expanded={expanded}
        visible={visibleColumns.includes(field.name)}
        onExpandedChange={(open) => setExpandedField(open ? field.name : null)}
        onToggleColumn={onToggleColumn}
        onInsertField={onInsertField}
      >
        {expanded && (
          <RumFieldValuesBlock
            field={field}
            source={source}
            language={language}
            isValueFiltered={isValueFiltered}
            onFilterValue={onFilterValue}
          />
        )}
      </RumFieldRow>
    );
  };

  return (
    <CollapsibleSidePanel
      title={t('explorer.fields.title')}
      collapsed={collapsed}
      onCollapsedChange={onCollapsedChange}
      variant="utility"
      widthClassName="w-[260px]"
      resizable
      defaultWidth={260}
      resizeLabel={t('explorer.fields.resize')}
      bodyClassName="flex flex-col"
      // The head row lines up with the head of the results beside the list.
      headerClassName="h-11"
      // The search stands in the head row, beside the button that folds the list: its words say what the list is.
      header={
        <SidePanelSearch
          value={fieldFilter}
          onChange={onFieldFilterChange}
          placeholder={t('explorer.fields.filter_placeholder')}
          label={t('explorer.fields.filter_aria')}
        />
      }
      {...(className ? { className } : {})}
      collapseLabel={t('explorer.fields.collapse')}
      expandLabel={t('explorer.fields.expand')}
      footer={
        <div className="flex h-11 items-center truncate px-3 font-sans text-xs text-tx-3">
          {t('explorer.fields.summary', { shown: filtered.length, total: fields.length })}
        </div>
      }
    >
      <TooltipProvider>
        {/* A flat list from edge to edge, as the table beside it: parts under bands like the table's head, a field a row. */}
        <div className="min-h-0 flex-1 overflow-auto">
          {sections.map(
            (section) =>
              section.nodes.length > 0 && (
                <SidePanelSection
                  key={section.key}
                  title={t(`explorer.fields.${section.key}`)}
                  count={fieldTreeSize(section.nodes)}
                  variant="band"
                >
                  {section.nodes.map((node) => {
                    if (node.kind === 'field') return renderRow(node);

                    const holdsOpenField = node.children.some((child) => child.field.name === expandedField);

                    return (
                      <FieldGroup
                        key={`group:${node.name}`}
                        variant="flat"
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
            <div className="px-2 py-4 text-center font-sans text-xs text-tx-3">{t('explorer.fields.no_match')}</div>
          )}
        </div>
      </TooltipProvider>
    </CollapsibleSidePanel>
  );
}

const ROW_ACTION = 'grid h-7 w-7 shrink-0 place-items-center rounded text-tx-3 hover:bg-bg-4 focus-visible:bg-bg-4';
/**
 * What can be asked of a field stands at the end of its row while the pointer is on the row or the
 * focus is in it, and takes no room otherwise: a name is cut short for buttons that are there, not
 * for the place where they would be.
 */
const ROW_ACTIONS = 'hidden shrink-0 items-center group-hover:flex group-focus-within:flex';

function RumFieldRow({
  field,
  label,
  nested,
  expandable,
  expanded,
  visible,
  onExpandedChange,
  onToggleColumn,
  onInsertField,
  children,
}: {
  field: RumField;
  /** The name the row is listed under: the field's own, or the rest of it inside a group. */
  label: string;
  /** The row is one of a group's, on the group's ground. */
  nested: boolean;
  expandable: boolean;
  expanded: boolean;
  visible: boolean;
  onExpandedChange: (open: boolean) => void;
  onToggleColumn: (name: string) => void;
  onInsertField: (field: RumField) => void;
  /** The field's values, when it is open. */
  children: React.ReactNode;
}) {
  const { t } = useTranslation('rum');
  const valuesId = React.useId();
  const queryable = isRumFieldQueryable(field);
  const toggleLabel = visible
    ? t('explorer.fields.hide_field_aria', { name: field.name })
    : t('explorer.fields.show_field_aria', { name: field.name });
  const addLabel = queryable
    ? t('explorer.fields.add_field_aria', { name: field.name })
    : t('explorer.fields.json_query_required', { name: field.name });

  return (
    <div data-rum-field={field.name} data-field-row className={FIELD_FLAT_ROW_CLASS}>
      <div
        data-rum-field-row
        className={cn('group flex items-center gap-1 pr-1', FIELD_FLAT_HEAD_CLASS, nested ? 'pl-8' : 'pl-3')}
      >
        <button
          type="button"
          onClick={() => onExpandedChange(!expanded)}
          disabled={!expandable}
          className={cn(
            'flex min-w-0 flex-1 items-center gap-2 text-left focus-visible:bg-transparent disabled:cursor-default',
            FIELD_FLAT_LINE_CLASS,
          )}
          aria-expanded={expandable ? expanded : undefined}
          aria-controls={expanded ? valuesId : undefined}
        >
          <FieldTypeChip glyph={TYPE_GLYPH[field.dataType]} tone={TYPE_TONE[field.dataType]} nested={nested} />
          <span className="min-w-0 truncate font-body text-tx-0" title={field.name}>
            {label}
          </span>
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
        <div className={ROW_ACTIONS}>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() => onToggleColumn(field.name)}
                className={cn(ROW_ACTION, 'hover:text-tx-0 focus-visible:text-tx-0')}
                aria-label={toggleLabel}
                aria-pressed={visible}
              >
                {visible ? <Eye className="h-3 w-3" /> : <EyeOff className="h-3 w-3" />}
              </button>
            </TooltipTrigger>
            <TooltipContent>{toggleLabel}</TooltipContent>
          </Tooltip>
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
          data-rum-field-values={field.name}
          // What a field opens on is under its row, on a ground a step off the list.
          className={cn(FIELD_FLAT_NEST_CLASS, nested && 'pl-8')}
        >
          {children}
        </div>
      )}
    </div>
  );
}
