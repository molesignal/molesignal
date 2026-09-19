import { Play, RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import {
  ChromeButton,
  TimeRangeChip,
} from '@/shell/chrome';
import type { CodeCompletionItem } from '@/shell/codeEditor/types';
import { QueryEditorFrame } from '@/shell/query/EditorFrame';
import { QuerySyntaxHelp } from '@/shell/query/SyntaxHelp';
import { useSqlFunctionCompletions } from '@/shell/query/useSqlFunctionCompletions';
import {
  QueryToolbarButton,
  QueryToolbarGroup,
  QueryToolbarTabs,
  QueryWorkbench,
  type QueryToolbarTab,
} from '@/shell/query/Workbench';

import {
  type ParsedTraceStatement,
  type TraceQueryMode,
} from '../fieldQueryModel';
import type { TraceTab } from './types';

const TRACE_QUERY_MODES: Array<{ id: TraceQueryMode; labelKey: string }> = [
  { id: 'fields', labelKey: 'explore.query.modes.fields' },
  { id: 'sql', labelKey: 'explore.query.modes.sql' },
];

interface TraceQueryPanelProps {
  tab: TraceTab;
  tabs: Array<QueryToolbarTab<TraceTab>>;
  queryMode: TraceQueryMode;
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
  onQueryModeChange: (mode: TraceQueryMode) => void;
  onQueryDraftChange: (value: string) => void;
  onSqlDraftChange: (value: string) => void;
  onCollapsedChange: (collapsed: boolean) => void;
  onApplySearch: () => void;
  onRefresh: () => void;
}

export function TraceQueryPanel({
  tab,
  tabs,
  queryMode,
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
  onQueryModeChange,
  onQueryDraftChange,
  onSqlDraftChange,
  onCollapsedChange,
  onApplySearch,
  onRefresh,
}: TraceQueryPanelProps) {
  const { t } = useTranslation('traces');
  const activeDraft = queryMode === 'sql' ? sqlDraft : queryDraft;
  const isQueryTab = tab === 'spans' || tab === 'traces';
  // SQL 检索函数（MATCH / MATCH_TEXT）由后端能力驱动，仅 SQL 模式注入（fields 走 q 全文搜索）。
  const sqlFunctions = useSqlFunctionCompletions();

  return (
    <QueryWorkbench
      appearance="surface"
      className="shrink-0"
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
              <QueryToolbarGroup
                aria-label={t('explore.query.mode_aria')}
                className="border-0 bg-[var(--control-surface)]"
              >
                {TRACE_QUERY_MODES.map((item) => (
                  <QueryToolbarButton
                    key={item.id}
                    active={queryMode === item.id}
                    tone="indigo"
                    flat
                    onClick={() => onQueryModeChange(item.id)}
                  >
                    {t(item.labelKey)}
                  </QueryToolbarButton>
                ))}
              </QueryToolbarGroup>
              <QuerySyntaxHelp
                mode={queryMode}
                scope="traces"
                compact
                className="border-0"
              />
            </>
          )}
          <div className="ml-auto flex flex-wrap items-center justify-end gap-1.5">
            <TimeRangeChip className="border-0" />
            {isQueryTab && (
              <ChromeButton
                variant="primary"
                onClick={onApplySearch}
                disabled={running || !canRun}
              >
                <Play className="h-3 w-3" aria-hidden="true" />
                {running ? t('explore.query.running') : t('explore.query.run')}
              </ChromeButton>
            )}
            <ChromeButton onClick={onRefresh} disabled={running}>
              <RefreshCw className="h-3 w-3" /> {t('explore.toolbar.refresh')}
            </ChromeButton>
          </div>
        </>
      }
    >
      {isQueryTab && (
        <>
          <QueryEditorFrame
            queryRef="A"
            value={activeDraft}
            onChange={queryMode === 'sql' ? onSqlDraftChange : onQueryDraftChange}
            onClear={() => {
              if (queryMode === 'sql') onSqlDraftChange('');
              else onQueryDraftChange('');
            }}
            clearLabel={t('explore.query.clear_query')}
            onModEnter={() => {
              if (!running && canRun) onApplySearch();
            }}
            language={queryMode === 'sql' ? 'sql' : 'field-query'}
            ariaLabel={queryMode === 'sql' ? 'Trace SQL query editor' : 'Trace field query editor'}
            placeholder={queryMode === 'sql' ? sqlPlaceholder : 'trace_id = "..." / service_name contains "checkout"'}
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
          />
          {!collapsed && queryMode === 'fields' && (queryText || parsedQuery.filters.length > 0 || parsedQuery.rejected.length > 0) && (
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
                <span key={item} className="inline-flex h-6 items-center rounded-md bg-yellow-dim px-2 text-yellow-soft">
                  ignored: {item}
                </span>
              ))}
            </div>
          )}
        </>
      )}
    </QueryWorkbench>
  );
}
