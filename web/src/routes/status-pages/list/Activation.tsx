import { Plus } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { StatusArt } from '@/shell/firstRun/art';
import { FirstRun } from '@/shell/firstRun/FirstRun';

/** What the module shows before there is a status page: what one is, and the way to the first. */
export function StatusPagesActivation({
  manageAccess,
  onCreate,
  onShowArchived,
}: {
  manageAccess: ActionAccess;
  onCreate: () => void;
  onShowArchived: () => void;
}) {
  const { t } = useTranslation('status-pages');

  return (
    <FirstRun
      data-testid="status-pages-first-run"
      title={t('first_run.title')}
      description={t('first_run.description')}
      tone="var(--green)"
      illustration={<StatusArt />}
      actions={
        <>
          <ChromeButton
            variant="primary"
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onClick={onCreate}
          >
            <Plus className="h-3.5 w-3.5" />
            {t('actions.create_page')}
          </ChromeButton>
          <ChromeButton onClick={onShowArchived}>{t('first_run.archived')}</ChromeButton>
        </>
      }
      steps={[
        { title: t('first_run.steps.components.title'), description: t('first_run.steps.components.description') },
        { title: t('first_run.steps.publish.title'), description: t('first_run.steps.publish.description') },
        { title: t('first_run.steps.subscribers.title'), description: t('first_run.steps.subscribers.description') },
      ]}
    />
  );
}
