import {
  ChevronDown,
  ChevronUp,
  Code2,
  ExternalLink,
  Play,
  Plus,
  RefreshCw,
  Search,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton, TimeRangeChip } from '@/shell/chrome';
import type { CodeCompletionItem } from '@/shell/codeEditor/types';
import { QueryEditorFrame } from '@/shell/query/EditorFrame';
import { QuerySyntaxHelp } from '@/shell/query/SyntaxHelp';
import { TimezoneSelect } from '@/shell/TimezoneSelect';

import type { MetricsQueryOptions } from './queryOptions/model';
import { QueryOptionsEditor } from './queryOptions/QueryOptionsEditor';

interface ExploreQuerySectionProps {
  promql: string;
  completionItems: CodeCompletionItem[];
  collapsed: boolean;
  mode: 'code' | 'builder';
  builder: React.ReactNode;
  canRun: boolean;
  running: boolean;
  dirty: boolean;
  timezone: string;
  options: MetricsQueryOptions;
  addToDashboardDisabled: boolean;
  addToDashboardDisabledReason?: React.ReactNode;
  promqlDocsHref: string;
  onPromqlChange: (value: string) => void;
  onCollapsedChange: (collapsed: boolean) => void;
  onModeChange: (mode: 'code' | 'builder') => void;
  onOpenMetricBrowser: () => void;
  onTimezoneChange: (timezone: string) => void;
  onOptionsChange: (options: MetricsQueryOptions) => void;
  onRun: () => void;
  onRefresh: () => void;
  onAddToDashboard: () => void;
}

export function ExploreQuerySection({
  promql,
  completionItems,
  collapsed,
  mode,
  builder,
  canRun,
  running,
  dirty,
  timezone,
  options,
  addToDashboardDisabled,
  addToDashboardDisabledReason,
  promqlDocsHref,
  onPromqlChange,
  onCollapsedChange,
  onModeChange,
  onOpenMetricBrowser,
  onTimezoneChange,
  onOptionsChange,
  onRun,
  onRefresh,
  onAddToDashboard,
}: ExploreQuerySectionProps) {
  const { t } = useTranslation('metrics');
  const runLabel = running
    ? t('explore.toolbar.running')
    : t('explore.editor.run');

  return (
    <section className="shrink-0 bg-bg-0 px-3 pt-3">
      <div
        className="flex min-h-12 flex-wrap items-center gap-2 rounded-md border border-bd-0 bg-bg-1 px-2 py-1.5 lg:flex-nowrap"
        data-testid="metrics-explore-toolbar"
      >
        <QuerySyntaxHelp
          mode="promql"
          scope="metrics"
          ariaLabel={t('explore.function_hint.aria')}
          triggerTitle={t('explore.function_hint.title')}
          title={t('explore.function_hint.title')}
          description={t('explore.function_hint.description')}
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
              href={promqlDocsHref}
              target="_blank"
              rel="noopener noreferrer"
              aria-label={t('explore.function_hint.docs_aria')}
              className="inline-flex items-center gap-1 font-semibold text-blue-soft underline-offset-2 hover:underline focus-visible:bg-bg-3 focus-visible:text-blue"
            >
              {t('explore.function_hint.docs')}
              <ExternalLink className="h-3 w-3" aria-hidden="true" />
            </a>
          )}
        />

        <div className="ml-auto flex flex-1 flex-wrap items-center justify-end gap-1.5 lg:flex-none">
          <TimeRangeChip />
          <TimezoneSelect
            value={timezone}
            onChange={onTimezoneChange}
            className="h-11 sm:h-9"
          />
          <ChromeButton
            variant="primary"
            onClick={onRun}
            disabled={!canRun}
            className="h-11 sm:h-9"
            data-query-dirty={dirty || undefined}
          >
            {dirty ? (
              <span className="h-1.5 w-1.5 rounded-full bg-orange-soft" aria-hidden="true" />
            ) : null}
            <Play className="h-3.5 w-3.5" aria-hidden="true" />
            {runLabel}
          </ChromeButton>
          <ChromeButton
            onClick={onRefresh}
            disabled={!canRun}
            className="h-11 sm:h-9"
            aria-label={t('explore.toolbar.refresh')}
          >
            <RefreshCw className="h-3.5 w-3.5" aria-hidden="true" />
            <span className="hidden xl:inline">
              {running
                ? t('explore.toolbar.running')
                : t('explore.toolbar.refresh')}
            </span>
          </ChromeButton>
          <ChromeButton
            onClick={onAddToDashboard}
            disabled={addToDashboardDisabled}
            disabledReason={addToDashboardDisabledReason}
            className="h-11 sm:h-9"
            aria-label={t('explore.toolbar.add_to_dashboard')}
          >
            <Plus className="h-3.5 w-3.5" aria-hidden="true" />
            <span className="hidden 2xl:inline">
              {t('explore.toolbar.add_to_dashboard')}
            </span>
          </ChromeButton>
        </div>
      </div>

      <div
        className="mt-2 overflow-hidden rounded-md border border-bd-1 bg-bg-1"
        data-testid="metrics-query-card"
      >
        <div className="flex min-h-11 flex-wrap items-center gap-2 border-b border-bd-0 bg-bg-2/70 px-2 py-1.5">
          <span className="grid h-7 min-w-7 place-items-center rounded border border-bd-1 bg-bg-1 px-1 font-mono text-xs font-bold text-tx-0">
            A
          </span>
          <div className="flex min-w-0 items-center gap-2">
            <span className="hidden font-mono text-xs text-tx-3 sm:inline">
              {t('explore.editor.label')}
            </span>
          </div>

          <div className="ml-auto flex items-center gap-1">
            <div
              role="group"
              aria-label={t('explore.query.mode_aria')}
              className="flex h-11 items-center rounded-md border border-bd-0 bg-bg-1 p-0.5 sm:h-8"
            >
              <button
                type="button"
                aria-pressed={mode === 'code'}
                onClick={() => onModeChange('code')}
                className={`inline-flex h-full items-center gap-1.5 rounded px-2.5 font-sans text-xs font-semibold transition-colors hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 ${
                  mode === 'code' ? 'bg-bg-4 text-tx-0' : 'text-tx-2'
                }`}
              >
                <Code2 className="h-3.5 w-3.5" aria-hidden="true" />
                {t('explore.query.code')}
              </button>
              <button
                type="button"
                aria-pressed={mode === 'builder'}
                onClick={() => onModeChange('builder')}
                className={`inline-flex h-full items-center rounded px-2.5 font-sans text-xs font-semibold transition-colors hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 ${
                  mode === 'builder' ? 'bg-bg-4 text-tx-0' : 'text-tx-2'
                }`}
              >
                {t('explore.query.builder')}
              </button>
            </div>
            <button
              type="button"
              onClick={() => onCollapsedChange(!collapsed)}
              aria-expanded={!collapsed}
              aria-label={
                collapsed
                  ? t('explore.toolbar.expand_editor')
                  : t('explore.toolbar.collapse_editor')
              }
              className="grid h-11 w-11 place-items-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 sm:h-8 sm:w-8"
            >
              {collapsed ? (
                <ChevronDown className="h-4 w-4" aria-hidden="true" />
              ) : (
                <ChevronUp className="h-4 w-4" aria-hidden="true" />
              )}
            </button>
          </div>
        </div>

        {collapsed ? (
          <button
            type="button"
            data-query-editor-state="collapsed"
            onClick={() => onCollapsedChange(false)}
            className="flex min-h-12 w-full min-w-0 items-center gap-3 px-3 text-left hover:bg-bg-2 focus-visible:bg-bg-2"
          >
            <code className="min-w-0 flex-1 truncate font-mono text-xs text-tx-2">
              {promql || t('explore.toolbar.empty_query_summary')}
            </code>
            <span className="shrink-0 font-sans text-xs font-semibold text-blue-soft">
              {t('explore.toolbar.expand_editor')}
            </span>
          </button>
        ) : (
          <div className="min-w-0">
            {mode === 'code' ? (
              <>
                <button
                  type="button"
                  data-testid="metric-search-trigger"
                  aria-haspopup="dialog"
                  onClick={onOpenMetricBrowser}
                  className="flex h-11 w-full items-center gap-2 border-b border-bd-0 bg-bg-1 px-3 text-left font-sans text-xs text-tx-3 transition-colors hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 focus-visible:text-tx-1"
                >
                  <Search className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
                  <span>{t('explore.catalog.search_trigger')}</span>
                </button>
                <QueryEditorFrame
                  queryRef="A"
                  value={promql}
                  onChange={onPromqlChange}
                  onClear={() => onPromqlChange('')}
                  clearLabel={t('explore.toolbar.clear_query')}
                  onModEnter={() => {
                    if (canRun) onRun();
                  }}
                  language="promql"
                  label={t('explore.editor.label')}
                  ariaLabel={t('explore.editor.aria')}
                  placeholder={t('explore.editor.placeholder')}
                  completionItems={completionItems}
                  minHeight={112}
                  maxHeight={240}
                  fontSize={13}
                  lineHeight={20}
                  lineNumbers
                  resizable
                  showHeader={false}
                  frameClassName="rounded-none border-0 shadow-none"
                  editorClassName="min-h-[112px]"
                />
              </>
            ) : (
              builder
            )}
            <QueryOptionsEditor value={options} onChange={onOptionsChange} />
          </div>
        )}
      </div>
    </section>
  );
}
