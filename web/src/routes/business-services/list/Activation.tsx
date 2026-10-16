import { Plus } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { StatusArt } from '@/shell/firstRun/art';
import { FirstRun } from '@/shell/firstRun/FirstRun';

/** What the module shows before there is a business service: what one is, and the way to the first. */
export function BusinessServicesActivation({
  manageAccess,
  onCreate,
}: {
  manageAccess: ActionAccess;
  onCreate: () => void;
}) {
  const { t } = useTranslation('business-services');

  return (
    <FirstRun
      data-testid="business-services-first-run"
      title={t('first_run.title')}
      description={t('first_run.description')}
      tone="var(--indigo)"
      illustration={<StatusArt />}
      actions={
        <ChromeButton
          variant="primary"
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onClick={onCreate}
        >
          <Plus className="h-3.5 w-3.5" />
          {t('actions.create')}
        </ChromeButton>
      }
      steps={[
        { title: t('first_run.steps.define.title'), description: t('first_run.steps.define.description') },
        { title: t('first_run.steps.depend.title'), description: t('first_run.steps.depend.description') },
        { title: t('first_run.steps.publish.title'), description: t('first_run.steps.publish.description') },
      ]}
    />
  );
}
