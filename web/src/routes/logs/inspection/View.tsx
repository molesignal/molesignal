import { useQuery } from '@tanstack/react-query';
import { Check, ExternalLink, Loader2, RotateCcw, ScanSearch, Square, TriangleAlert } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as providersApi from '@/api/agent/modelProviders';
import { writeClipboardText } from '@/lib/clipboard';
import { useProductAccess } from '@/product/access';
import { useActionAccess } from '@/product/actionAccess';
import { MarkdownMessage } from '@/routes/agent/markdown';
import { ChromeButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { EmptyState } from '@/shell/EmptyState';
import { FormField, FormTextarea } from '@/shell/FormDrawer';
import { QueryState } from '@/shell/query/State';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shell/ui/select';

import { canReadInspections } from '../inspections/access';
import { InspectionsLink, ScheduleInspectionButton } from '../inspections/Entry';
import type { LogEntry } from '../viewModel';
import { buildInspectionPrompt, digestLoadedLogs, type InspectionTarget } from './model';
import { InspectionReport, isReportInTheMaking } from './Report';
import type { InspectionTool, LogInspection } from './useInspection';

interface LogInspectionViewProps {
  /** What would be inspected: the stream, the query as it was run, and its time range. */
  target: InspectionTarget | null;
  /** The range as the reader sees it. */
  rangeLabel: string;
  /** The loaded logs: the model is told what they say at a glance. */
  rows: readonly LogEntry[];
  /**
   * The query matched nothing. There is nothing to inspect now, but the query
   * can still be saved to be inspected on a schedule.
   */
  empty?: boolean | undefined;
  inspection: LogInspection;
}

const TOOL_ICON: Record<InspectionTool['status'], React.ReactNode> = {
  running: <Loader2 className="h-3 w-3 animate-spin text-[var(--blue-ink)] motion-reduce:animate-none" aria-hidden="true" />,
  done: <Check className="h-3 w-3 text-[var(--green-ink)]" aria-hidden="true" />,
  error: <TriangleAlert className="h-3 w-3 text-[var(--orange-ink)]" aria-hidden="true" />,
};

/**
 * An inspection of the logs a query matches, by the model: an overview, what
 * stands out, and where to look next.
 *
 * It needs a model that is set up and switched on, and a reader who may use
 * Mole Agent. Where either is missing the view says which, and where it is put
 * right, instead of offering a button that would fail.
 */
export function LogInspectionView({
  target,
  rangeLabel,
  rows,
  empty = false,
  inspection,
}: LogInspectionViewProps) {
  const { t } = useTranslation('logs');
  const access = useProductAccess();
  const use = useActionAccess({ permission: 'agent.use' });
  const manage = useActionAccess({ permission: 'agent.manage' });
  const providers = useQuery({
    queryKey: ['agent', 'model-providers'],
    queryFn: () => providersApi.list(),
    enabled: use.allowed,
    retry: false,
  });
  const enabled = React.useMemo(() => (providers.data ?? []).filter((provider) => provider.enabled), [providers.data]);
  const [providerId, setProviderId] = React.useState('');
  const [requirements, setRequirements] = React.useState('');
  const [copied, setCopied] = React.useState(false);
  const provider = enabled.find((candidate) => candidate.id === providerId) ?? enabled[0];
  const { run } = inspection;

  if (!use.allowed) {
    return use.disabled ? (
      <EmptyState
        strategy="permission-denied"
        title={t('explore.inspection.unavailable.permission_title')}
        description={t('explore.inspection.unavailable.permission_description')}
        data-testid="log-inspection-no-permission"
      />
    ) : (
      <QueryState state="loading" loadingLabel={t('explore.inspection.unavailable.loading')} className="h-full" />
    );
  }
  if (providers.isPending) {
    return <QueryState state="loading" loadingLabel={t('explore.inspection.unavailable.loading')} className="h-full" />;
  }
  if (providers.isError) {
    return (
      <QueryState
        state="error"
        error={providers.error}
        errorLabel={t('explore.inspection.unavailable.failed_title')}
        className="h-full"
      />
    );
  }
  // No model, or none switched on: there is nobody to do the inspecting.
  if (!provider) {
    return (
      <EmptyState
        icon={ScanSearch}
        title={t('explore.inspection.unavailable.model_title')}
        description={
          manage.allowed
            ? t('explore.inspection.unavailable.model_description')
            : `${t('explore.inspection.unavailable.model_description')} ${t('explore.inspection.unavailable.ask_admin')}`
        }
        primaryAction={
          manage.allowed
            ? { label: t('explore.inspection.unavailable.configure'), to: '/agent/settings/models' }
            : undefined
        }
        // What is already saved to run on a schedule stays within reach without a model.
        secondaryAction={
          canReadInspections(access)
            ? { label: t('inspections.entry'), to: '/logs/inspections' }
            : undefined
        }
        data-testid="log-inspection-no-model"
      />
    );
  }

  const start = () => {
    if (!target) return;

    setCopied(false);
    inspection.start({
      provider,
      title: t('explore.inspection.chat_title', { stream: target.stream }),
      prompt: buildInspectionPrompt(t, { target, rangeLabel, requirements, digest: digestLoadedLogs(rows) }),
      stream: target.stream,
      range: target.range,
    });
  };
  const copyReport = async () => {
    try {
      await writeClipboardText(run.report);
      setCopied(true);
    } catch {
      // Clipboard access can be denied by the browser.
    }
  };
  const running = run.status === 'running';

  if (run.status === 'idle') {
    return (
      <div data-log-inspection="form" className="h-full overflow-auto">
        <div className="mx-auto flex w-full max-w-[720px] flex-col gap-4 px-4 py-5">
          <div>
            <div className="flex min-w-0 items-center gap-2">
              <h3 className="min-w-0 flex-1 font-sans text-sm font-strong text-tx-0">
                {t('explore.inspection.title')}
              </h3>
              <InspectionsLink />
            </div>
            <p className="mt-1 font-sans text-xs leading-relaxed text-tx-2">{t('explore.inspection.intro')}</p>
          </div>
          <FormField label={t('explore.inspection.target')}>
            <div
              data-log-inspection-target
              className="flex min-h-9 flex-wrap items-center gap-x-2 gap-y-1 rounded-md bg-[var(--control-surface)] px-3 py-2 font-sans text-xs text-tx-1"
            >
              <span className="font-strong text-tx-0">{target?.stream}</span>
              <span className="text-tx-3">·</span>
              <span>{rangeLabel}</span>
              <span className="text-tx-3">·</span>
              {/* A statement is set as one; "all logs" is a phrase, in the face of the page. */}
              {target?.statement.trim() ? (
                <code className="min-w-0 truncate font-code text-tx-2" title={target.statement}>
                  {target.statement.trim().replace(/\s+/g, ' ')}
                </code>
              ) : (
                <span className="text-tx-2">{t('explore.inspection.target_all')}</span>
              )}
            </div>
          </FormField>
          {enabled.length > 1 && (
            <FormField label={t('explore.inspection.model')}>
              <Select value={provider.id} onValueChange={setProviderId}>
                <SelectTrigger aria-label={t('explore.inspection.model')}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {enabled.map((candidate) => (
                    <SelectItem key={candidate.id} value={candidate.id} className="font-sans text-sm">
                      {candidate.name} · {candidate.default_model}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormField>
          )}
          <FormField label={t('explore.inspection.requirements')} hint={t('explore.inspection.requirements_hint')}>
            <FormTextarea
              value={requirements}
              onChange={(event) => setRequirements(event.target.value)}
              rows={5}
              maxLength={2_000}
              placeholder={t('explore.inspection.requirements_placeholder')}
            />
          </FormField>
          <div className="flex flex-wrap items-center gap-3">
            <ChromeButton
              variant="primary"
              onClick={start}
              disabled={!target || empty}
              disabledReason={empty ? t('explore.inspection.no_logs') : undefined}
              data-log-inspection-start
            >
              <ScanSearch className="h-3.5 w-3.5" aria-hidden="true" />
              {t('explore.inspection.start')}
            </ChromeButton>
            {target && (
              <ScheduleInspectionButton
                seed={{
                  stream: target.stream,
                  statement: target.statement,
                  requirements,
                  providerId: provider.id,
                }}
              />
            )}
            {/* What happens to the logs is said before the button is pressed. */}
            <p className="min-w-0 flex-1 basis-full font-sans text-xs text-tx-3 sm:basis-0">
              {empty
                ? t('explore.inspection.no_logs')
                : t('explore.inspection.disclosure', { model: provider.default_model })}
            </p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div data-log-inspection={run.status} className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-2 px-4 pb-2 pt-1">
        <h3 className="min-w-0 flex-1 truncate font-sans text-sm font-strong text-tx-0">
          {t(running ? 'explore.inspection.running' : 'explore.inspection.report')}
        </h3>
        {running ? (
          <ChromeButton size="sm" onClick={inspection.stop}>
            <Square className="h-3 w-3" aria-hidden="true" />
            {t('explore.inspection.stop')}
          </ChromeButton>
        ) : (
          <>
            {run.chatId && (
              <Link
                to="/agent/chat"
                className="inline-flex h-8 items-center gap-1.5 rounded-md px-2.5 font-sans text-xs font-strong text-[var(--blue-ink)] hover:bg-bg-2 focus-visible:bg-bg-3"
              >
                <ExternalLink className="h-3 w-3" aria-hidden="true" />
                {t('explore.inspection.continue_in_agent')}
              </Link>
            )}
            <ChromeButton size="sm" onClick={inspection.reset}>
              <RotateCcw className="h-3 w-3" aria-hidden="true" />
              {t('explore.inspection.again')}
            </ChromeButton>
            <CopyIconButton
              onClick={copyReport}
              disabled={!run.report}
              label={t('explore.inspection.copy_report')}
              copied={copied}
            />
          </>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-4 pb-4">
        <div className="mx-auto flex w-full max-w-[860px] flex-col gap-3">
          {run.tools.length > 0 && (
            <ul data-log-inspection-tools className="flex flex-col gap-1 rounded-md bg-[var(--control-surface)] px-3 py-2">
              {run.tools.map((tool) => (
                <li key={tool.id} className="flex items-center gap-2 font-sans text-xs text-tx-2">
                  {TOOL_ICON[tool.status]}
                  <span className="min-w-0 truncate">
                    {t(`explore.inspection.tool_${tool.status}`, { name: tool.name })}
                  </span>
                </li>
              ))}
            </ul>
          )}
          {run.status === 'failed' && (
            <p role="alert" className="rounded-md bg-[var(--orange-tint)] px-3 py-2 font-sans text-xs text-[var(--orange-ink)]">
              {t('explore.inspection.failed')}
              {run.error ? `: ${run.error}` : ''}
            </p>
          )}
          {running ? (
            // An answer in the agent's fixed shape arrives as half an object: it is shown once it
            // is whole. Prose is read as it is written.
            run.report && !isReportInTheMaking(run.report) ? (
              <MarkdownMessage content={run.report} streaming className="font-sans text-sm text-tx-1" />
            ) : (
              <QueryState
                state="loading"
                loadingLabel={t(run.report ? 'explore.inspection.writing' : 'explore.inspection.waiting')}
                className="min-h-32"
              />
            )
          ) : (
            run.report && <InspectionReport content={run.report} />
          )}
        </div>
      </div>
    </div>
  );
}
