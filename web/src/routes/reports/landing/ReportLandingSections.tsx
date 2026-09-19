import { CircleAlert, LayoutTemplate, Plus, type LucideIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
import { DisabledControl } from '@/shell/DisabledControl';

import { rangeLabel, templateIcon, type TemplatePreset } from '../reportTypes';

export function ReportStarter({
  templates,
  onUseTemplate,
  onCustom,
}: {
  templates: TemplatePreset[];
  onUseTemplate: (template: TemplatePreset) => void;
  onCustom: () => void;
}) {
  const { t } = useTranslation('reports');
  const scheduleAccess = useActionAccess({ permission: 'reports.schedule' });

  return (
    <section aria-labelledby="report-starter-title">
      <div className="flex max-w-3xl items-start gap-3 px-4 py-4">
        <span className="grid h-10 w-10 shrink-0 place-items-center rounded-md bg-indigo-dim text-indigo-soft">
          <LayoutTemplate className="h-5 w-5" aria-hidden="true" />
        </span>
        <div>
          <h2
            id="report-starter-title"
            className="m-0 font-sans text-lg font-display-strong tracking-tight text-tx-0"
          >
            {t('empty.title')}
          </h2>
          <p className="mt-1 max-w-2xl font-sans text-sm leading-6 text-tx-2">
            {t('empty.description')}
          </p>
        </div>
      </div>

      <div className="grid gap-[12px] p-4 lg:grid-cols-4">
        {templates.map((template) => (
          <StarterOption
            key={template.id}
            title={template.name}
            description={template.description}
            cadence={rangeLabel(template.rangePreset, t)}
            icon={templateIcon(template.icon)}
            disabled={scheduleAccess.disabled}
            disabledReason={scheduleAccess.reason}
            actionLabel={t('actions.use_template')}
            onClick={() => onUseTemplate(template)}
          />
        ))}

        <DisabledControl
          disabled={scheduleAccess.disabled}
          reason={scheduleAccess.reason}
          className="w-full"
        >
          <button
            type="button"
            className="group min-h-[168px] w-full rounded-md bg-[var(--control-surface)] px-4 py-4 text-left transition-colors enabled:hover:bg-bg-3 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-60"
            disabled={scheduleAccess.disabled}
            aria-disabled={scheduleAccess.disabled || undefined}
            onClick={onCustom}
          >
            <span className="grid h-9 w-9 place-items-center rounded-md bg-bg-2 text-indigo-soft transition-colors group-hover:bg-indigo-dim">
              <Plus className="h-4 w-4" aria-hidden="true" />
            </span>
            <span className="mt-4 block font-sans text-sm font-semibold text-tx-0">
              {t('empty.custom_title')}
            </span>
            <span className="mt-1 block font-sans text-sm leading-5 text-tx-2">
              {t('empty.custom_description')}
            </span>
            <span className="mt-3 block font-sans text-xs font-semibold text-indigo-soft">
              {t('actions.new_report')}
            </span>
          </button>
        </DisabledControl>
      </div>
    </section>
  );
}

function StarterOption({
  title,
  description,
  cadence,
  icon: Icon,
  disabled,
  disabledReason,
  actionLabel,
  onClick,
}: {
  title: string;
  description: string;
  cadence: string;
  icon: LucideIcon;
  disabled: boolean;
  disabledReason?: string | undefined;
  actionLabel: string;
  onClick: () => void;
}) {
  return (
    <DisabledControl disabled={disabled} reason={disabledReason} className="w-full">
      <button
        type="button"
        className="group min-h-[168px] w-full rounded-md bg-[var(--control-surface)] px-4 py-4 text-left transition-colors enabled:hover:bg-bg-3 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-60"
        disabled={disabled}
        aria-disabled={disabled || undefined}
        onClick={onClick}
      >
        <span className="grid h-9 w-9 place-items-center rounded-md bg-bg-2 text-indigo-soft transition-colors group-hover:bg-indigo-dim">
          <Icon className="h-4 w-4" aria-hidden="true" />
        </span>
        <span className="mt-4 block font-sans text-sm font-semibold text-tx-0">{title}</span>
        <span className="mt-1 line-clamp-2 block font-sans text-sm leading-5 text-tx-2">
          {description}
        </span>
        <span className="mt-3 flex items-center gap-2">
          <Pill tone="neutral">{cadence}</Pill>
          <span className="font-sans text-xs font-semibold text-indigo-soft">{actionLabel}</span>
        </span>
      </button>
    </DisabledControl>
  );
}

export function TemplateLibrary({
  templates,
  apiError,
  onRetry,
  onUse,
  onEdit,
}: {
  templates: TemplatePreset[];
  apiError: boolean;
  onRetry: () => void;
  onUse: (template: TemplatePreset) => void;
  onEdit: (template: TemplatePreset) => void;
}) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const scheduleAccess = useActionAccess({ permission: 'reports.schedule' });
  const editAccess = useActionAccess({ permission: 'reports.edit' });

  return (
    <section aria-label={t('tabs.templates')}>
      {apiError && (
        <div
          role="status"
          className="m-4 flex flex-wrap items-center gap-3 rounded-md bg-yellow-dim px-4 py-3 text-sm text-yellow-soft"
        >
          <CircleAlert className="h-4 w-4 shrink-0" aria-hidden="true" />
          <span className="min-w-0 flex-1">{t('templates.api_warning')}</span>
          <ChromeButton size="sm" onClick={onRetry}>
            {tc('actions.retry')}
          </ChromeButton>
        </div>
      )}

      <div className="space-y-2 p-4">
        {templates.map((template) => {
          const Icon = templateIcon(template.icon);
          return (
            <article
              key={template.id}
              className="group grid gap-4 rounded-md bg-[var(--control-surface)] px-4 py-4 transition-colors hover:bg-bg-3 lg:grid-cols-[40px_minmax(0,1fr)_minmax(190px,auto)_auto] lg:items-center"
            >
              <span className="grid h-10 w-10 place-items-center rounded-md bg-bg-2 text-indigo-soft transition-colors group-hover:bg-indigo-dim">
                <Icon className="h-4.5 w-4.5" aria-hidden="true" />
              </span>

              <div className="min-w-0">
                <h3 className="m-0 truncate font-sans text-sm font-semibold text-tx-0">
                  {template.name}
                </h3>
                <p className="mb-0 mt-1 line-clamp-2 font-sans text-sm leading-5 text-tx-2">
                  {template.description}
                </p>
              </div>

              <div className="flex flex-wrap items-center gap-1.5 lg:justify-end">
                <Pill tone={template.isBuiltin ? 'dim' : 'indigo'}>
                  {template.isBuiltin
                    ? t('templates.builtin_badge')
                    : t('templates.custom_badge')}
                </Pill>
                <Pill tone="neutral">
                  {t('templates.source_badge', {
                    source: t(`source.${template.sourceKind}`),
                  })}
                </Pill>
                <Pill tone="dim">{template.format.toUpperCase()}</Pill>
                <Pill tone="dim">{rangeLabel(template.rangePreset, t)}</Pill>
              </div>

              <div className="flex items-center gap-2 lg:justify-end">
                {!template.isBuiltin && (
                  <ChromeButton
                    size="sm"
                    variant="ghost"
                    disabled={editAccess.disabled}
                    disabledReason={editAccess.reason}
                    onClick={() => onEdit(template)}
                  >
                    {t('actions.edit')}
                  </ChromeButton>
                )}
                <ChromeButton
                  size="sm"
                  disabled={scheduleAccess.disabled}
                  disabledReason={scheduleAccess.reason}
                  onClick={() => onUse(template)}
                >
                  {t('actions.use_template')}
                </ChromeButton>
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}
