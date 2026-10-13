import { RefreshCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { TimeRangeChip } from '@/shell/chrome';
import type { CodeCompletionItem } from '@/shell/codeEditor/types';
import { FilterBuilder } from '@/shell/query/builder/FilterBuilder';
import { QueryEditorFrame } from '@/shell/query/EditorFrame';
import { QueryIconAction, QueryRunAction } from '@/shell/query/IconAction';
import { type QueryEditorMode, QueryModeSwitch } from '@/shell/query/ModeSwitch';
import { QueryShareButton } from '@/shell/query/share/Button';
import { QuerySyntaxHelp } from '@/shell/query/SyntaxHelp';
import { useSqlFunctionCompletions } from '@/shell/query/useSqlFunctionCompletions';
import {
  QueryToolbarTabs,
  QueryWorkbench,
  type QueryToolbarTab,
} from '@/shell/query/Workbench';
import { WorkbenchTitle } from '@/shell/SurfaceWorkbench';

import { traceBuilderDialect, traceBuilderFields } from '../builderDialect';
import {
  type ParsedTraceStatement,
  type TraceFieldDef,
  type TraceQueryMode,
} from '../fieldQueryModel';
import type { TraceTab } from './types';

// The hint in an empty Code editor: a field query, or SQL on the next line.
const FIELD_QUERY_HINT = 'trace_id = "..." / service_name contains "checkout"';

interface TraceQueryPanelProps {
  tab: TraceTab;
  tabs: Array<QueryToolbarTab<TraceTab>>;
  /** How the query is written: as a statement, or put together from filters. */
  editorMode: QueryEditorMode;
  /** The language of the statement in use; the Code editor takes either. */
  queryMode: TraceQueryMode;
  traceFields: TraceFieldDef[];
  /** A field picked in the field list while the builder is open. */
  builderField: string | null;
  queryDraft: string;
  queryText: string;
  sqlDraft: string;
  parsedQuery: ParsedTraceStatement;
  completionItems: CodeCompletionItem[];
  running: boolean;
  canRun: boolean;
  collapsed: boolean;
  sqlPlaceholder: string;
  onTabChange: (tab: TraceTab) => void;
  onEditorModeChange: (mode: QueryEditorMode) => void;
  /** A statement typed in Code; what is typed decides which language it is. */
  onCodeDraftChange: (value: string) => void;
  /** The field query as the builder wrote it. */
  onBuilderDraftChange: (value: string) => void;
  onBuilderFieldHandled: () => void;
  onCollapsedChange: (collapsed: boolean) => void;
  onApplySearch: () => void;
  onRefresh: () => void;
  /** The query on screen as the page's own search params say it: what a link to it carries. */
  shareSearch: URLSearchParams;
}

export function TraceQueryPanel({
  tab,
  tabs,
  editorMode,
  queryMode,
  traceFields,
  builderField,
  queryDraft,
  queryText,
  sqlDraft,
  parsedQuery,
  completionItems,
  running,
  canRun,
  collapsed,
  sqlPlaceholder,
  onTabChange,
  onEditorModeChange,
  onCodeDraftChange,
  onBuilderDraftChange,
  onBuilderFieldHandled,
  onCollapsedChange,
  onApplySearch,
  onRefresh,
  shareSearch,
}: TraceQueryPanelProps) {
  const { t } = useTranslation('traces');
  const { t: tCommon } = useTranslation('common');
  const activeDraft = queryMode === 'sql' ? sqlDraft : queryDraft;
  const builderFields = React.useMemo(() => traceBuilderFields(traceFields), [traceFields]);
  const builderDialect = React.useMemo(() => traceBuilderDialect(traceFields), [traceFields]);
  const runIfReady = () => {
    if (!running && canRun) onApplySearch();
  };
  const isQueryTab = tab === 'spans' || tab === 'traces';
  // SQL 检索函数（MATCH / MATCH_TEXT）由后端能力驱动，仅 SQL 模式注入（fields 走 q 全文搜索）。
  const sqlFunctions = useSqlFunctionCompletions();

  return (
    <QueryWorkbench
      appearance="surface"
      className="mt-[6px] shrink-0"
      inlineBody={isQueryTab && collapsed}
      // No title row on this page: the bar carries the name and the row goes to the data.
      title={<WorkbenchTitle title={t('explore.title')} />}
      {...(!isQueryTab ? { bodyClassName: 'hidden' } : {})}
      toolbar={
        <>
          <QueryToolbarTabs
            tabs={tabs}
            activeId={tab}
            onChange={onTabChange}
            tone="indigo"
            selectionStyle="underline"
          />
          {isQueryTab && (
            <>
              <QueryModeSwitch mode={editorMode} onChange={onEditorModeChange} />
              <QuerySyntaxHelp
                mode={queryMode}
                scope="traces"
                compact
                className="border-0"
              />
            </>
          )}
        </>
      }
      actions={
        <>
          <TimeRangeChip className="border-0" />
          {isQueryTab && (
            <QueryRunAction
              label={t('explore.query.run')}
              runningLabel={t('explore.query.running')}
              running={running}
              onClick={onApplySearch}
              disabled={running || !canRun}
            />
          )}
          {/* As on Logs, refreshing is a sign without a word: the bar has to keep the folded query on its row. */}
          <QueryIconAction
            icon={RefreshCw}
            label={t('explore.toolbar.refresh')}
            onClick={onRefresh}
            disabled={running}
          />
          <QueryShareButton search={shareSearch} />
        </>
      }
    >
      {isQueryTab && (
        <>
          <QueryEditorFrame
            queryRef="A"
            value={activeDraft}
            // One editor for both languages: what is typed says which it is.
            onChange={onCodeDraftChange}
            onClear={() => onCodeDraftChange('')}
            clearLabel={t('explore.query.clear_query')}
            // The shortcut belongs to the editor; the builder runs on Enter in its text box.
            {...(editorMode === 'code' ? { onModEnter: runIfReady } : {})}
            language={queryMode === 'sql' ? 'sql' : 'field-query'}
            // One name whatever is typed: the mode is on the switch beside it, the language under the editor.
            label={tCommon('query_language.label')}
            languageBadge={false}
            ariaLabel={queryMode === 'sql' ? 'Trace SQL query editor' : 'Trace field query editor'}
            placeholder={`${FIELD_QUERY_HINT}\n${sqlPlaceholder.split('\n')[0] ?? ''}`.trim()}
            collapsed={collapsed}
            onCollapsedChange={onCollapsedChange}
            collapseLabel={t('explore.query.collapse_editor')}
            expandLabel={t('explore.query.expand_editor')}
            summary={activeDraft || t('explore.query.empty_summary')}
            completionItems={queryMode === 'fields' ? completionItems : sqlFunctions}
            frameClassName="border-0 bg-[var(--control-surface)] shadow-none"
            minHeight={160}
            maxHeight={320}
            lineNumbers
            resizable
            body={editorMode === 'builder' ? (
              <FilterBuilder
                statement={queryDraft}
                onStatementChange={onBuilderDraftChange}
                dialect={builderDialect}
                fields={builderFields}
                pendingField={builderField}
                onPendingFieldHandled={onBuilderFieldHandled}
                onRun={runIfReady}
                notice={queryMode === 'sql' ? tCommon('query_builder.sql_active') : undefined}
              />
            ) : undefined}
          />
          {!collapsed && editorMode === 'code' && queryMode === 'fields' && (queryText || parsedQuery.filters.length > 0 || parsedQuery.rejected.length > 0) && (
            <div className="mt-2 flex flex-wrap gap-2 font-sans text-xs">
              {parsedQuery.q && (
                <span className="inline-flex h-6 items-center rounded-md bg-[var(--control-surface)] px-2 text-tx-1">
                  {t('explore.query.free_text_label')} {parsedQuery.q}
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
                  {t('explore.query.ignored_label')} {item}
                </span>
              ))}
            </div>
          )}
        </>
      )}
    </QueryWorkbench>
  );
}
