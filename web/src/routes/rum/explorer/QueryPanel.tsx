import { RefreshCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumField } from '@/api/rum';
import { TimeRangeChip } from '@/shell/chrome';
import type { CodeCompletionItem } from '@/shell/codeEditor/types';
import { FilterBuilder } from '@/shell/query/builder/FilterBuilder';
import { QueryEditorFrame } from '@/shell/query/EditorFrame';
import { QueryIconAction, QueryRunAction } from '@/shell/query/IconAction';
import { type QueryEditorMode, QueryModeSwitch } from '@/shell/query/ModeSwitch';
import { QueryShareButton } from '@/shell/query/share/Button';
import { QuerySyntaxHelp } from '@/shell/query/SyntaxHelp';
import { QueryToolbarTabs, QueryWorkbench, type QueryToolbarTab } from '@/shell/query/Workbench';
import { useQueryTabStrip } from '@/shell/queryTabs';
import { WorkbenchTitle } from '@/shell/SurfaceWorkbench';

import { rumBuilderDialect, rumBuilderFields } from './builderDialect';
import type { ParsedRumStatement } from './fieldQuery';
import type { RumTab } from './model';

// The hint in an empty Code editor.
const FIELD_QUERY_HINT = "usr.id = 'u-42' / view.url_path contains 'checkout'";

interface RumQueryPanelProps {
  tab: RumTab;
  tabs: Array<QueryToolbarTab<RumTab>>;
  /** How the query is written: as a statement, or put together from filters. */
  editorMode: QueryEditorMode;
  fields: readonly RumField[];
  /** A field picked in the field list while the builder is open. */
  builderField: string | null;
  queryDraft: string;
  queryText: string;
  parsedQuery: ParsedRumStatement;
  completionItems: CodeCompletionItem[];
  running: boolean;
  collapsed: boolean;
  onTabChange: (tab: RumTab) => void;
  onEditorModeChange: (mode: QueryEditorMode) => void;
  onDraftChange: (value: string) => void;
  onBuilderFieldHandled: () => void;
  onCollapsedChange: (collapsed: boolean) => void;
  onApplySearch: () => void;
  onRefresh: () => void;
  /** The query on screen as the page's own search params say it: what a link to it carries. */
  shareSearch: URLSearchParams;
  /** The views of the explorer one keeps to come back to. */
  savedViews: React.ReactNode;
  /** What is drawn, put on a dashboard or watched by an alert rule. */
  exports?: React.ReactNode;
  /** The row under the query that says what is counted of its rows, when they are drawn as numbers. */
  aggregation?: React.ReactNode;
}

/** The bar over the explorer: the types, the query and what is done with it, the query tabs in the query's box. */
export function RumQueryPanel({
  tab,
  tabs,
  editorMode,
  fields,
  builderField,
  queryDraft,
  queryText,
  parsedQuery,
  completionItems,
  running,
  collapsed,
  onTabChange,
  onEditorModeChange,
  onDraftChange,
  onBuilderFieldHandled,
  onCollapsedChange,
  onApplySearch,
  onRefresh,
  shareSearch,
  savedViews,
  exports,
  aggregation,
}: RumQueryPanelProps) {
  const { t } = useTranslation('rum');
  const { t: tCommon } = useTranslation('common');
  const builderFields = React.useMemo(() => rumBuilderFields(fields), [fields]);
  const queryTabs = useQueryTabStrip();
  const runIfReady = () => {
    if (!running) onApplySearch();
  };
  const examples = React.useMemo(
    () => [
      { label: t('explorer.query.example_equals'), expression: "usr.id = 'u-42'" },
      { label: t('explorer.query.example_contains'), expression: "view.url_path contains 'checkout'" },
      { label: t('explorer.query.example_exclude'), expression: "browser.name != 'Safari'" },
      { label: t('explorer.query.example_number'), expression: 'resource.duration >= 1000000000' },
    ],
    [t],
  );

  return (
    <QueryWorkbench
      appearance="surface"
      // Right under the module's tabs, which keep the room every RUM page has under them.
      className="shrink-0"
      inlineBody={collapsed}
      // The slot under the query: a row of its own there, for what is counted of what it finds.
      notice={aggregation}
      // No title row on this page: the bar carries the name and the row goes to the data.
      title={<WorkbenchTitle title={t('explorer.title')} />}
      toolbar={
        <>
          <QueryToolbarTabs
            tabs={tabs}
            activeId={tab}
            onChange={onTabChange}
            tone="indigo"
            selectionStyle="underline"
          />
          <QueryModeSwitch mode={editorMode} onChange={onEditorModeChange} />
          <QuerySyntaxHelp
            mode="fields"
            scope="logs"
            compact
            className="border-0"
            examples={examples}
            title={t('explorer.query.help_title')}
            description={t('explorer.query.help_description')}
          />
        </>
      }
      actions={
        <>
          <TimeRangeChip className="border-0" />
          <QueryRunAction
            label={t('explorer.query.run')}
            runningLabel={t('explorer.query.running')}
            running={running}
            onClick={onApplySearch}
            disabled={running}
          />
          <QueryIconAction
            icon={RefreshCw}
            label={t('explorer.toolbar.refresh')}
            onClick={onRefresh}
            disabled={running}
          />
          {exports}
          <QueryShareButton search={shareSearch} />
          {savedViews}
        </>
      }
    >
      <QueryEditorFrame
        queryRef="A"
        tabs={queryTabs}
        value={queryDraft}
        onChange={onDraftChange}
        onClear={() => onDraftChange('')}
        clearLabel={t('explorer.query.clear_query')}
        // The shortcut belongs to the editor; the builder runs on Enter in its text box.
        {...(editorMode === 'code' ? { onModEnter: runIfReady } : {})}
        language="field-query"
        label={tCommon('query_language.label')}
        languageBadge={false}
        ariaLabel="RUM field query editor"
        placeholder={FIELD_QUERY_HINT}
        collapsed={collapsed}
        onCollapsedChange={onCollapsedChange}
        collapseLabel={t('explorer.query.collapse_editor')}
        expandLabel={t('explorer.query.expand_editor')}
        summary={queryDraft || t('explorer.query.empty_summary', { type: t(`explorer.types.${tab}`).toLocaleLowerCase() })}
        completionItems={completionItems}
        frameClassName="border-0 bg-[var(--control-surface)] shadow-none"
        minHeight={160}
        maxHeight={320}
        lineNumbers
        resizable
        body={
          editorMode === 'builder' ? (
            <FilterBuilder
              statement={queryDraft}
              onStatementChange={onDraftChange}
              dialect={rumBuilderDialect}
              fields={builderFields}
              pendingField={builderField}
              onPendingFieldHandled={onBuilderFieldHandled}
              onRun={runIfReady}
            />
          ) : undefined
        }
      />
      {!collapsed && editorMode === 'code' && (queryText || parsedQuery.filters.length > 0 || parsedQuery.rejected.length > 0) && (
        <div className="mt-2 flex flex-wrap gap-2 font-sans text-xs">
          {parsedQuery.freeText.length > 0 && (
            <span className="inline-flex h-6 items-center rounded-md bg-[var(--control-surface)] px-2 text-tx-1">
              {t('explorer.query.free_text_label')} {parsedQuery.freeText.join(' ')}
            </span>
          )}
          {parsedQuery.filters.map((filter, index) => (
            <span
              key={`${filter.field}-${filter.op}-${filter.value}-${index}`}
              className="inline-flex h-6 items-center rounded-md bg-[var(--control-surface)] px-2 text-tx-1"
            >
              {filter.field} {filter.op} {filter.value}
            </span>
          ))}
          {parsedQuery.rejected.map((item) => (
            <span key={item} className="inline-flex h-6 items-center rounded-md bg-[var(--yellow-tint)] px-2 text-yellow">
              {t('explorer.query.ignored_label')} {item}
            </span>
          ))}
        </div>
      )}
    </QueryWorkbench>
  );
}
