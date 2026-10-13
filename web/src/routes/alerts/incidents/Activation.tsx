import { Plus } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { ThresholdArt } from '@/shell/firstRun/art';
import { FirstRun } from '@/shell/firstRun/FirstRun';

/** What Alerts shows before there is a rule: nothing can fire until something is watched. */
export function AlertsActivation() {
  const { t } = useTranslation('alerts');
  const navigate = useNavigate();
  const createAccess = useActionAccess({ permission: 'alerts.manage' });

  return (
    <FirstRun
      data-testid="alerts-first-run"
      title={t('center.incidents.no_rules.title')}
      description={t('center.incidents.no_rules.description')}
      tone="var(--red)"
      illustration={<ThresholdArt />}
      actions={
        <ChromeButton
          variant="primary"
          disabled={createAccess.disabled}
          disabledReason={createAccess.reason}
          onClick={() => navigate('/alerts/rules/new')}
        >
          <Plus className="h-4 w-4" />
          {t('center.incidents.no_rules.action')}
        </ChromeButton>
      }
      steps={(['condition', 'route', 'respond'] as const).map((step) => ({
        title: t(`center.incidents.no_rules.steps.${step}.title`),
        description: t(`center.incidents.no_rules.steps.${step}.description`),
      }))}
    />
  );
}
