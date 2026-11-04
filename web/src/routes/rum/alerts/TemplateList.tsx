import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumApplication } from '@/api/rum';
import type { ActionAccess } from '@/product/actionAccess';
import { rumDefaultMinSamples, rumSampleKind } from '@/routes/alerts/rule-workbench/rumSource';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { Skeleton } from '@/shell/ui/skeleton';
import type { AlertRule, RumAlertMetric } from '@/types/alerting';

import { TEXT_LINK } from './GoingOn';
import { createdRule, formatFigure, gradeOfFigure, operatorSymbol, tripped } from './model';
import { type AlertTemplate, templateRule, templatesFor, type TemplateScope } from './templates';
import { useCreateRule, useCurrentFigure } from './useRumAlerts';
import { isBrowser } from '../overview/model';
import { GRADE_TEXT } from '../performance/ink';
import { Section } from '../performance/PanelSection';

/** The templates that are told by one sentence, because their lines are counts of what is new or back. */
const SENTENCE = new Set(['new_issues', 'regressions', 'traffic']);

/** The figures whose name says more than the template's title does: a percentile, or what a share is of. */
const TAGGED = new Set<RumAlertMetric>(['lcp_p75', 'inp_p75', 'cls_p75', 'error_rate', 'crash_free_sessions']);

/**
 * The rules a template of the application would make, each as a card: what it watches and where it
 * pages, what the figure is now, and one button that makes the rule. A rule that exists says so in
 * place of the button. The rules are read before any card can create one, so a template is never
 * made twice.
 */
export function TemplateList({
  application,
  environment,
  rules,
  ready,
  manage,
}: {
  application: RumApplication;
  environment: string;
  rules: readonly AlertRule[] | undefined;
  /** Whether the rules are read: until they are, nobody knows what already exists. */
  ready: boolean;
  manage: ActionAccess;
}) {
  const { t } = useTranslation('rum');
  const browser = isBrowser(application.platform);

  return (
    <Section
      title={t('alerts.templates.title')}
      pending={false}
      failed={false}
      onRetry={() => undefined}
      empty={false}
      emptyText=""
      testId="alerts-templates"
    >
      <p className="m-0 text-xs text-tx-3">{t(browser ? 'alerts.templates.hint_browser' : 'alerts.templates.hint_app')}</p>
      <ul data-rum-alert-templates="" className="m-0 flex list-none flex-col p-0">
        {templatesFor(application.platform).map((template) => (
          <TemplateCard key={template.id} template={template} application={application} environment={environment} rules={rules} ready={ready} manage={manage} />
        ))}
      </ul>
    </Section>
  );
}

function TemplateCard({
  template,
  application,
  environment,
  rules,
  ready,
  manage,
}: {
  template: AlertTemplate;
  application: RumApplication;
  environment: string;
  rules: readonly AlertRule[] | undefined;
  ready: boolean;
  manage: ActionAccess;
}) {
  const { t, i18n } = useTranslation('rum');
  const { t: ta } = useTranslation('alerts');
  const language = i18n.resolvedLanguage ?? i18n.language;
  const scope: TemplateScope = { application: application.id, environment };
  const made = createdRule(rules, template, scope);
  const reading = useCurrentFigure(template, scope, ready && !made);
  const create = useCreateRule();

  const title = t(`alerts.templates.items.${template.id}.title`, isBrowser(application.platform) ? {} : { context: 'app' });
  const name = environment
    ? t('alerts.templates.name_environment', { application: application.name, environment, title })
    : t('alerts.templates.name', { application: application.name, title });
  const period = ta(`workbench.rum.windows.${template.window}`);
  const rows = rumDefaultMinSamples(template.metric);
  const kind = rumSampleKind(template.metric);

  const line = SENTENCE.has(template.id)
    ? t(`alerts.templates.items.${template.id}.rule`, {
        where: environment || t('alerts.templates.any_environment'),
        severity: ta(`severity.${template.bands[0]!.severity}`),
        window: period,
      })
    : [
        t('alerts.templates.line', {
          bands: template.bands
            .map((entry) => `${ta(`severity.${entry.severity}`)} ${operatorSymbol(entry.operator)} ${formatFigure(template.metric, entry.threshold, language)}`)
            .join(t('alerts.templates.list_separator')),
          window: period,
        }),
        rows > 0 && kind ? t('alerts.templates.at_least', { rows, kind: ta(`workbench.rum.sample_kinds.${kind}`) }) : null,
      ]
        .filter(Boolean)
        .join(' · ');

  return (
    <li
      data-rum-alert-template={template.id}
      data-rum-alert-created={made ? '' : undefined}
      className="flex items-start gap-3 border-t border-bd-0 py-2.5 text-xs first:border-t-0 first:pt-0"
    >
      <div className="min-w-0 flex-1 space-y-0.5">
        <div className="flex flex-wrap items-baseline gap-x-2">
          <span className="font-strong text-tx-0">{title}</span>{' '}
          {TAGGED.has(template.metric) && <span className="text-tx-3">{ta(`workbench.rum.metrics.${template.metric}`)}</span>}{' '}
          {template.recommended && <span className="text-[var(--blue-ink)]">{t('alerts.templates.recommended')}</span>}{' '}
          {template.optional && <span className="text-tx-3">{t('alerts.templates.optional')}</span>}
        </div>
        <div className="text-tx-3">{line}</div>
        <div data-rum-alert-now="">
          {made ? (
            <span className="inline-flex flex-wrap items-baseline gap-x-3">
              <span className={cn('font-strong', made.enabled ? 'text-[var(--green-ink)]' : 'text-tx-3')}>
                ✓ {t(made.enabled ? 'alerts.templates.created' : 'alerts.templates.created_off')}
              </span>{' '}
              {manage.allowed && (
                <Link to={`/alerts/rules/${encodeURIComponent(made.id)}/edit`} className={TEXT_LINK}>
                  {t('alerts.templates.view')}
                </Link>
              )}
            </span>
          ) : (
            <Now template={template} reading={reading} period={period} kind={kind} language={language} />
          )}
        </div>
      </div>
      {!made && (
        <ChromeButton
          size="sm"
          data-rum-alert-create={template.id}
          disabled={!ready || create.isPending || manage.disabled}
          disabledReason={manage.disabled ? manage.reason : undefined}
          onClick={() => create.mutate(templateRule(template, scope, name))}
        >
          {t(create.isPending ? 'alerts.templates.creating' : 'alerts.templates.create')}
        </ChromeButton>
      )}
    </li>
  );
}

/** What the figure is now, and what a rule made from the template would do about it at once. */
function Now({
  template,
  reading,
  period,
  kind,
  language,
}: {
  template: AlertTemplate;
  reading: ReturnType<typeof useCurrentFigure>;
  /** The window the figure is read over, in words. */
  period: string;
  kind: ReturnType<typeof rumSampleKind>;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const { t: ta } = useTranslation('alerts');

  if (reading.isPending) return <Skeleton aria-hidden className="mt-1 h-3 w-28" />;

  const answer = reading.data;

  if (reading.isError || !answer || answer.unavailable) {
    return <span className="text-tx-3">{t('alerts.templates.unreadable')}</span>;
  }

  const current = answer.current;

  if (!current || current.value === null) {
    return (
      <span data-rum-alert-no-value="" className="text-tx-3">
        {t('alerts.templates.no_value', { rows: answer.minSamples, kind: kind ? ta(`workbench.rum.sample_kinds.${kind}`) : '', window: period })}
      </span>
    );
  }

  const grade = gradeOfFigure(template.metric, current.value);
  const trip = tripped(template.bands, current.value);

  return (
    <span>
      <span className="text-tx-3">{t('alerts.templates.now')}</span>{' '}
      <span data-rum-alert-value="" className="font-strong tabular-nums text-tx-0">
        {template.metric === 'session_count'
          ? t('alerts.templates.sessions_per', { count: current.value, window: period })
          : formatFigure(template.metric, current.value, language)}
      </span>
      {grade && (
        <>
          {' '}
          <span className={cn('font-strong', GRADE_TEXT[grade])}>{t(`overview.vitals.grade.${grade}`)}</span>
        </>
      )}
      {trip && (
        <>
          {' '}
          <span className="ml-1.5 text-tx-3">{t('alerts.templates.would_trip', { severity: ta(`severity.${trip}`) })}</span>
        </>
      )}
    </span>
  );
}
