import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { FieldGroup } from '@/shell/query/fields/Group';
import { fieldTree, type FieldTreeLeaf } from '@/shell/query/fields/tree';

import { isLogFieldFilterable, type LogFieldDef } from '../fieldQueryModel';
import type { LogQueryMode } from '../queryState';
import { logFieldAnalysisKind } from './analysisKind';
import { LogFieldRow } from './Row';

interface LogFieldListProps {
  fields: LogFieldDef[];
  /**
   * Fields that share the first part of their names are listed under it. Not
   * for the fields on display, whose order is the order of the columns.
   */
  grouped?: boolean;
  /** A search has cut the list to what matches: every group stands open on it. */
  searching?: boolean;
  /** Whether these fields are columns of the list. */
  visible: boolean;
  queryMode: LogQueryMode;
  /** The loaded records: an open row lists the field's most common values among them. */
  records: Record<string, unknown>[];
  expandedField: string | null;
  onExpandedFieldChange: (field: string | null) => void;
  /** The field whose analysis is open beside the list. */
  analysedField?: string | null | undefined;
  /** Opens a field's analysis, or closes it with `null`. Absent while there are no results to analyse. */
  onAnalysedFieldChange?: ((field: string | null) => void) | undefined;
  onToggleVisibility: (field: string) => void;
  onInsertFilter: (field: string) => void;
  onInsertValueFilter: (field: string, value: unknown, mode: 'include' | 'exclude') => void;
}

/**
 * The fields of one section of the log explorer's field list, one row each;
 * where asked, as a tree: the fields of `http` under `http`.
 */
export function LogFieldList({
  fields,
  grouped = false,
  searching = false,
  visible,
  queryMode,
  records,
  expandedField,
  onExpandedFieldChange,
  analysedField = null,
  onAnalysedFieldChange,
  onToggleVisibility,
  onInsertFilter,
  onInsertValueFilter,
}: LogFieldListProps) {
  const { t } = useTranslation('common');
  const tree = React.useMemo(
    () => (grouped ? fieldTree(fields) : fields.map((field) => ({ kind: 'field' as const, field, label: field.name }))),
    [fields, grouped],
  );
  const [openGroups, setOpenGroups] = React.useState<ReadonlySet<string>>(() => new Set());
  const setGroupOpen = (name: string, open: boolean) =>
    setOpenGroups((current) => {
      const next = new Set(current);
      if (open) next.add(name);
      else next.delete(name);

      return next;
    });

  const row = ({ field, label }: FieldTreeLeaf<LogFieldDef>, nested: boolean) => (
    <LogFieldRow
      key={field.name}
      field={field}
      label={label}
      nested={nested}
      filterable={isLogFieldFilterable(field, queryMode)}
      visible={visible}
      expanded={expandedField === field.name}
      records={records}
      analysis={
        onAnalysedFieldChange && logFieldAnalysisKind(field)
          ? {
              open: analysedField === field.name,
              onToggle: () => onAnalysedFieldChange(analysedField === field.name ? null : field.name),
            }
          : undefined
      }
      onExpandedChange={(open) => onExpandedFieldChange(open ? field.name : null)}
      onToggleVisibility={onToggleVisibility}
      onInsertFilter={onInsertFilter}
      onInsertValueFilter={onInsertValueFilter}
    />
  );

  return (
    <>
      {tree.map((node) =>
        node.kind === 'group' ? (
          <FieldGroup
            key={`group:${node.name}`}
            name={node.name}
            count={node.children.length}
            // A field that is open on its values keeps its group open around it.
            open={
              searching ||
              openGroups.has(node.name) ||
              node.children.some((child) => child.field.name === expandedField)
            }
            onOpenChange={(open) => {
              setGroupOpen(node.name, open);
              if (!open && node.children.some((child) => child.field.name === expandedField)) {
                onExpandedFieldChange(null);
              }
            }}
            label={t('field_list.group_aria', { name: node.name, count: node.children.length })}
          >
            {node.children.map((child) => row(child, true))}
          </FieldGroup>
        ) : (
          row(node, false)
        ),
      )}
    </>
  );
}
