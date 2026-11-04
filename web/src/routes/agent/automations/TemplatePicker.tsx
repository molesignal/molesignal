import { useQuery } from '@tanstack/react-query';
import { ArrowRight } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import * as automationsApi from '@/api/agent/automations';
import { ProductState } from '@/product/states';
import { FormDrawer } from '@/shell/FormDrawer';

import { draftFromTemplate, newDraft, triggerSummary, type AutomationDraft } from './model';

/**
 * Where a new automation starts: from nothing, or from one of the built-in
 * templates (incident analysis, the daily health check, the weekly reviews, the
 * change check a CI/CD job calls), which fill in the task and the trigger.
 */
export function TemplatePicker({
  open,
  timezone,
  onClose,
  onPick,
}: {
  open: boolean;
  timezone: string;
  onClose: () => void;
  onPick: (draft: AutomationDraft) => void;
}) {
  const { t } = useTranslation('agent');
  const templates = useQuery({
    queryKey: ['agent', 'automation-templates', timezone],
    queryFn: () => automationsApi.listAutomationTemplates(timezone),
    enabled: open,
    retry: false,
    staleTime: 5 * 60_000,
  });

  if (!open) return null;

  return (
    <FormDrawer
      open
      onOpenChange={(next) => !next && onClose()}
      title={t('automations.templates.title')}
      subtitle={t('automations.templates.subtitle')}
      width={560}
    >
      <ul data-automation-templates className="m-0 flex list-none flex-col gap-[6px] p-0">
        <li>
          <Choice
            name={t('automations.templates.blank')}
            description={t('automations.templates.blank_description')}
            onClick={() => onPick(newDraft(timezone))}
            data="blank"
          />
        </li>
        {templates.isPending ? (
          <li>
            <ProductState variant="loading" />
          </li>
        ) : templates.isError ? (
          <li>
            <ProductState variant="error" error={templates.error} />
          </li>
        ) : (
          templates.data.map((template) => (
            <li key={template.key}>
              <Choice
                name={template.name}
                description={template.description}
                detail={triggerSummary(t, template.trigger)}
                onClick={() => onPick(draftFromTemplate(template, timezone))}
                data={template.key}
              />
            </li>
          ))
        )}
      </ul>
    </FormDrawer>
  );
}

function Choice({
  name,
  description,
  detail,
  data,
  onClick,
}: {
  name: string;
  description: string;
  detail?: string;
  data: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      data-automation-template={data}
      onClick={onClick}
      className="group flex w-full items-start gap-3 rounded-md bg-bg-2 px-[14px] py-[12px] text-left font-sans transition-colors hover:bg-bg-3 focus-visible:bg-bg-3 focus-visible:outline-none"
    >
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-strong text-tx-0">{name}</span>
        <span className="mt-1 block text-xs leading-relaxed text-tx-2">{description}</span>
        {detail && <span className="mt-1.5 block text-xs text-tx-1">{detail}</span>}
      </span>
      <ArrowRight aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0 text-tx-3 group-hover:text-tx-1" />
    </button>
  );
}
