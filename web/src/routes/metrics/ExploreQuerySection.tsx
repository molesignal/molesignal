import {
  ChevronUp,
  ExternalLink,
  Plus,
  RefreshCw,
  Search,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { TimeRangeChip } from '@/shell/chrome';
import type { CodeCompletionItem } from '@/shell/codeEditor/types';
import { QueryEditorFrame } from '@/shell/query/EditorFrame';
import { QueryIconAction, QueryRunAction } from '@/shell/query/IconAction';
import { type QueryEditorMode, QueryModeSwitch } from '@/shell/query/ModeSwitch';
import { QueryShareButton } from '@/shell/query/share/Button';
import { QuerySyntaxHelp } from '@/shell/query/SyntaxHelp';
import { QueryWorkbench } from '@/shell/query/Workbench';
import { WorkbenchTitle } from '@/shell/SurfaceWorkbench';

import type { MetricsQueryOptions } from './queryOptions/model';
import { QueryOptionsEditor } from './queryOptions/QueryOptionsEditor';

interface ExploreQuerySectionProps {
  promql: string;
  completionItems: CodeCompletionItem[];
  collapsed: boolean;
  mode: QueryEditorMode;
  builder: React.ReactNode;
  canRun: boolean;
  running: boolean;
  dirty: boolean;
  options: MetricsQueryOptions;
  addToDashboardDisabled: boolean;
  addToDashboardDisabledReason?: React.ReactNode;
  promqlDocsHref: string;
  onPromqlChange: (value: string) => void;
  onCollapsedChange: (collapsed: boolean) => void;
  onModeChange: (mode: QueryEditorMode) => void;
  onOpenMetricBrowser: () => void;
  onOptionsChange: (options: MetricsQueryOptions) => void;
  onRun: () => void;
  onRefresh: () => void;
  onAddToDashboard: () => void;
  /** The query on screen as the page's own search params say it: what a link to it carries. */
  shareSearch: URLSearchParams;
}

/**
 * The Metrics workbench: one bar that names the page and holds the query mode,
 * the time range and the run actions, with the query under it.
 *
 * Collapsed, the query is one line and sits in the bar itself, so the chart
 * gets every row below it.
 */
export function ExploreQuerySection({
  promql,
  completionItems,
  collapsed,
  mode,
  builder,
  canRun,
  running,
  dirty,
  options,
  addToDashboardDisabled,
  addToDashboardDisabledReason,
  promqlDocsHref,
  onPromqlChange,
  onCollapsedChange,
  onModeChange,
  onOpenMetricBrowser,
  onOptionsChange,
  onRun,
  onRefresh,
  onAddToDashboard,
  shareSearch,
}: ExploreQuerySectionProps) {
  const { t } = useTranslation('metrics');
  const runIfReady = () => {
    if (canRun) onRun();
  };

  return (
    <QueryWorkbench
      appearance="surface"
      className="mt-[6px] shrink-0"
      inlineBody={collapsed}
      // No title row on this page: the bar carries the name and the row goes to the chart.
      title={<WorkbenchTitle title={t('explore.title')} />}
      toolbar={
        <>
          <QueryModeSwitch mode={mode} onChange={onModeChange} />
          <PromqlHelp docsHref={promqlDocsHref} />
        </>
      }
      actions={
        <>
          <TimeRangeChip className="border-0" />
          {/* As on Logs and Traces, every action of the bar is a sign without a word. */}
          <QueryRunAction
            label={t('explore.editor.run')}
            runningLabel={t('explore.toolbar.running')}
            running={running}
            // The query has been edited since it ran: a dot in the corner says the plot is not of it.
            changed={dirty}
            onClick={onRun}
            disabled={!canRun}
            data-query-dirty={dirty || undefined}
          />
          <QueryIconAction
            icon={RefreshCw}
            label={t('explore.toolbar.refresh')}
            onClick={onRefresh}
            disabled={!canRun}
          />
          <QueryIconAction
            icon={Plus}
            label={t('explore.toolbar.add_to_dashboard')}
            onClick={onAddToDashboard}
            disabled={addToDashboardDisabled}
            disabledReason={addToDashboardDisabledReason}
          />
          <QueryShareButton search={shareSearch} />
        </>
      }
    >
      <div data-testid="metrics-query-card">
        {collapsed ? (
          <QueryEditorFrame
            queryRef="A"
            value={promql}
            onChange={onPromqlChange}
            language="promql"
            label={t('explore.editor.label')}
            collapsed
            onCollapsedChange={onCollapsedChange}
            expandLabel={t('explore.toolbar.expand_editor')}
            summary={promql || t('explore.toolbar.empty_query_summary')}
            frameClassName="border-0 bg-[var(--control-surface)] shadow-none"
          />
        ) : (
          <div className="overflow-hidden rounded-md bg-[var(--control-surface)]">
            <div className="flex min-h-11 items-center gap-2 px-2 py-1.5">
              <span className="grid h-7 min-w-7 shrink-0 place-items-center rounded bg-[var(--functional-surface)] px-1 font-mono text-xs font-bold text-tx-0">
                A
              </span>
              {mode === 'code' ? (
                <button
                  type="button"
                  data-testid="metric-search-trigger"
                  aria-haspopup="dialog"
                  onClick={onOpenMetricBrowser}
                  className="flex h-11 min-w-0 flex-1 items-center gap-2 rounded-md bg-[var(--functional-surface)] px-3 text-left font-sans text-xs text-tx-3 transition-colors hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 focus-visible:text-tx-1 sm:h-8"
                >
                  <Search className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
                  <span className="truncate">{t('explore.catalog.search_trigger')}</span>
                </button>
              ) : (
                <span className="min-w-0 flex-1 truncate font-mono text-xs text-tx-3">
                  {t('explore.editor.label')}
                </span>
              )}
              <button
                type="button"
                onClick={() => onCollapsedChange(true)}
                aria-expanded
                aria-label={t('explore.toolbar.collapse_editor')}
                title={t('explore.toolbar.collapse_editor')}
                className="grid h-11 w-11 shrink-0 place-items-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 sm:h-8 sm:w-8"
              >
                <ChevronUp className="h-4 w-4" aria-hidden="true" />
              </button>
            </div>

            {mode === 'code' ? (
              <QueryEditorFrame
                queryRef="A"
                value={promql}
                onChange={onPromqlChange}
                onClear={() => onPromqlChange('')}
                clearLabel={t('explore.toolbar.clear_query')}
                onModEnter={runIfReady}
                language="promql"
                label={t('explore.editor.label')}
                ariaLabel={t('explore.editor.aria')}
                placeholder={t('explore.editor.placeholder')}
                completionItems={completionItems}
                minHeight={76}
                maxHeight={240}
                lineNumbers
                resizable
                showHeader={false}
                frameClassName="rounded-none border-0 shadow-none"
                editorClassName="min-h-[76px]"
              />
            ) : (
              builder
            )}
            <QueryOptionsEditor value={options} onChange={onOptionsChange} />
          </div>
        )}
      </div>
    </QueryWorkbench>
  );
}

function PromqlHelp({ docsHref }: { docsHref: string }) {
  const { t } = useTranslation('metrics');

  return (
    <QuerySyntaxHelp
      mode="promql"
      scope="metrics"
      ariaLabel={t('explore.function_hint.aria')}
      triggerTitle={t('explore.function_hint.title')}
      title={t('explore.function_hint.title')}
      description={t('explore.function_hint.description')}
      className="border-0"
      contentClassName="w-[480px]"
      examples={[
        {
          label: 'rate',
          expression: 'rate(metric_name[5m])',
          description: t('explore.function_hint.functions.rate'),
        },
        {
          label: 'increase',
          expression: 'increase(metric_name[5m])',
          description: t('explore.function_hint.functions.increase'),
        },
        {
          label: 'sum',
          expression: 'sum(metric_name)',
          description: t('explore.function_hint.functions.sum'),
        },
        {
          label: 'avg',
          expression: 'avg(metric_name)',
          description: t('explore.function_hint.functions.avg'),
        },
      ]}
      footer={(
        <a
          href={docsHref}
          target="_blank"
          rel="noopener noreferrer"
          aria-label={t('explore.function_hint.docs_aria')}
          className="inline-flex items-center gap-1 font-semibold text-[var(--blue-ink)] underline-offset-2 hover:underline focus-visible:bg-bg-3 focus-visible:text-blue"
        >
          {t('explore.function_hint.docs')}
          <ExternalLink className="h-3 w-3" aria-hidden="true" />
        </a>
      )}
    />
  );
}
