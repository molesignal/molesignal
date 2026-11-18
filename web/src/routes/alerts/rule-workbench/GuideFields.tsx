import { useQuery } from '@tanstack/react-query';
import { Plus, Trash2, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import * as dashboardsApi from '@/api/dashboards';
import { hasPermission, useProductAccess } from '@/product/access';
import { MarkdownEditor } from '@/routes/agent/markdownEditor/MarkdownEditor';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import { FormField, FormGroup, FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';

import {
  GUIDE_LIMITS,
  GUIDE_STREAM_TYPES,
  type GuideDraft,
  type GuideLanguage,
  type GuideProblem,
  type GuideQueryDraft,
  type GuideStreamType,
  newGuideQuery,
} from './guide';

/** The words for what keeps a guide from being saved, counting queries from one. */
export function guideProblemMessage(
  problem: GuideProblem,
  t: ReturnType<typeof useTranslation<'alerts'>>['t'],
): string {
  const place = 'index' in problem ? problem.index + 1 : 0;
  const limit = 'limit' in problem ? problem.limit : 0;
  return t(`workbench.errors.guide.${problem.kind}`, { index: place, limit });
}

/**
 * What to do first when one of the rule's incidents is looked into: the steps in
 * Markdown, the queries to run over the incident's time range, and the
 * dashboards to open beside it.
 */
export function GuideFields({
  value,
  onChange,
  readOnly = false,
}: {
  value: GuideDraft;
  onChange: (guide: GuideDraft) => void;
  readOnly?: boolean;
}) {
  const { t } = useTranslation('alerts');
  const access = useProductAccess();
  const canReadDashboards = hasPermission('dashboards.read', access);
  const dashboards = useQuery({
    queryKey: ['dashboards'],
    queryFn: () => dashboardsApi.list(),
    enabled: canReadDashboards,
    staleTime: 60_000,
  });

  const set = (patch: Partial<GuideDraft>) => onChange({ ...value, ...patch });
  const setQuery = (key: string, patch: Partial<GuideQueryDraft>) =>
    set({ queries: value.queries.map((query) => (query.key === key ? { ...query, ...patch } : query)) });
  const titleOf = (id: string) => dashboards.data?.find((dashboard) => dashboard.id === id)?.title ?? id;
  const addable = (dashboards.data ?? []).filter((dashboard) => !value.dashboards.includes(dashboard.id));

  return (
    <div data-rule-guide className="flex flex-col gap-4">
      <FormField label={t('workbench.guide.steps')} hint={t('workbench.guide.steps_hint')}>
        <MarkdownEditor
          value={value.steps}
          onChange={(steps) => set({ steps })}
          ariaLabel={t('workbench.guide.steps')}
          placeholder={t('workbench.guide.steps_placeholder')}
          className="min-h-[140px] rounded-md bg-[var(--control-surface)] px-3 py-2 transition-colors hover:bg-bg-3 focus-within:bg-bg-3"
        />
      </FormField>

      <FormGroup
        label={t('workbench.guide.queries')}
        hint={t('workbench.guide.queries_hint', { count: GUIDE_LIMITS.queries })}
      >
        {value.queries.map((query, index) => (
          <GuideQueryRow
            key={query.key}
            query={query}
            index={index}
            readOnly={readOnly}
            onChange={(patch) => setQuery(query.key, patch)}
            onRemove={() => set({ queries: value.queries.filter((candidate) => candidate.key !== query.key) })}
          />
        ))}
        <div>
          <ChromeButton
            onClick={() => set({ queries: [...value.queries, newGuideQuery()] })}
            disabled={readOnly || value.queries.length >= GUIDE_LIMITS.queries}
          >
            <Plus className="h-4 w-4" />
            {t('workbench.guide.add_query')}
          </ChromeButton>
        </div>
      </FormGroup>

      <FormGroup
        label={t('workbench.guide.dashboards')}
        hint={t('workbench.guide.dashboards_hint', { count: GUIDE_LIMITS.dashboards })}
      >
        {value.dashboards.length > 0 && (
          <ul className="flex flex-wrap gap-1.5">
            {value.dashboards.map((id) => (
              <li
                key={id}
                data-guide-dashboard
                className="flex min-w-0 items-center gap-1 rounded-md bg-bg-2 py-0.5 pl-2.5 pr-0.5 font-sans text-xs text-tx-0"
              >
                <span className="truncate">{titleOf(id)}</span>
                <IconButton
                  className="h-6 w-6"
                  aria-label={t('workbench.guide.remove_dashboard', { name: titleOf(id) })}
                  disabled={readOnly}
                  onClick={() => set({ dashboards: value.dashboards.filter((candidate) => candidate !== id) })}
                >
                  <X className="h-3.5 w-3.5" />
                </IconButton>
              </li>
            ))}
          </ul>
        )}
        {canReadDashboards && value.dashboards.length < GUIDE_LIMITS.dashboards && (
          <FormSelect
            value=""
            onChange={(id) => {
              if (id) set({ dashboards: [...value.dashboards, id] });
            }}
            options={addable.map((dashboard) => ({ value: dashboard.id, label: dashboard.title }))}
            placeholder={addable.length > 0 ? t('workbench.guide.add_dashboard') : t('workbench.guide.no_dashboards')}
            ariaLabel={t('workbench.guide.add_dashboard')}
            disabled={readOnly || addable.length === 0}
            className="max-w-[360px]"
          />
        )}
      </FormGroup>
    </div>
  );
}

/** One query of the guide: its name, language, stream for SQL, and statement. */
function GuideQueryRow({
  query,
  index,
  readOnly,
  onChange,
  onRemove,
}: {
  query: GuideQueryDraft;
  index: number;
  readOnly: boolean;
  onChange: (patch: Partial<GuideQueryDraft>) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation('alerts');
  const languages: GuideLanguage[] = ['promql', 'sql'];

  return (
    <div data-guide-query className="flex flex-col gap-3 bg-bg-2 px-3 py-3">
      <div className="flex items-end gap-2">
        <FormField label={t('workbench.guide.query_name')} required className="min-w-0 flex-1">
          <FormInput
            value={query.name}
            onChange={(event) => onChange({ name: event.target.value })}
            placeholder={t('workbench.guide.query_name_placeholder')}
            className="bg-[var(--control-surface)]"
          />
        </FormField>
        <div
          role="group"
          aria-label={t('workbench.guide.query_language')}
          className="flex h-8 w-fit shrink-0 items-center rounded-md bg-[var(--control-surface)] p-0.5"
        >
          {languages.map((language) => (
            <button
              key={language}
              type="button"
              aria-pressed={query.language === language}
              onClick={() => onChange({ language })}
              className={cn(
                'rounded-[5px] px-2.5 py-1 font-mono text-xs font-strong transition-colors',
                query.language === language ? 'bg-bg-0 text-indigo-soft' : 'text-tx-3 hover:text-tx-1',
              )}
            >
              {language}
            </button>
          ))}
        </div>
        <IconButton
          aria-label={t('workbench.guide.remove_query', { index: index + 1 })}
          disabled={readOnly}
          onClick={onRemove}
        >
          <Trash2 className="h-4 w-4" />
        </IconButton>
      </div>

      {query.language === 'sql' && (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-[minmax(0,1fr)_160px]">
          <FormField label={t('workbench.guide.query_stream')} required>
            <FormInput
              value={query.streamName}
              onChange={(event) => onChange({ streamName: event.target.value })}
              placeholder={t('workbench.placeholders.stream')}
              className="bg-[var(--control-surface)]"
            />
          </FormField>
          <FormField label={t('workbench.guide.query_stream_type')}>
            <FormSelect
              value={query.streamType}
              onChange={(streamType) => {
                if (streamType) onChange({ streamType: streamType as GuideStreamType });
              }}
              options={GUIDE_STREAM_TYPES.map((streamType) => ({
                value: streamType,
                label: t(`workbench.guide.stream_types.${streamType}`),
              }))}
            />
          </FormField>
        </div>
      )}

      <FormField label={t('workbench.guide.query_statement')} required>
        <CodeEditor
          value={query.statement}
          onChange={(statement) => onChange({ statement })}
          language={query.language}
          label={query.language === 'promql' ? 'PromQL' : 'SQL'}
          ariaLabel={t('workbench.guide.query_statement')}
          minHeight={72}
          maxHeight={240}
          compact
          resizable
          showHeader
          readOnly={readOnly}
        />
      </FormField>
    </div>
  );
}
