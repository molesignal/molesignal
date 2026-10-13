import { Plus } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { ProbeArt } from '@/shell/firstRun/art';
import { FirstRun } from '@/shell/firstRun/FirstRun';

// The checks most people begin with. Each opens the editor with its assertion in place.
const STARTING_POINTS = [
  ['http_code', 'status'],
  ['response_time', 'latency'],
  ['json', 'json'],
  ['tls', 'tls'],
  ['dns', 'dns'],
] as const;

/**
 * What Synthetics shows before there is a check: what a check is, and the way
 * to the first one.
 */
export function SyntheticsActivation() {
  const { t } = useTranslation('synthetics');
  const navigate = useNavigate();
  const createAccess = useActionAccess({ permission: 'synthetics.manage' });

  return (
    <FirstRun
      data-testid="synthetics-first-run"
      title={t('first_run.title')}
      description={t('first_run.description')}
      tone="var(--green)"
      illustration={<ProbeArt />}
      actions={
        <ChromeButton
          variant="primary"
          disabled={createAccess.disabled}
          disabledReason={createAccess.reason}
          onClick={() => navigate('/synthetics/checks/new')}
        >
          <Plus className="h-4 w-4" />
          {t('actions.create_check')}
        </ChromeButton>
      }
      steps={(['define', 'locate', 'notify'] as const).map((step) => ({
        title: t(`first_run.steps.${step}.title`),
        description: t(`first_run.steps.${step}.description`),
      }))}
    >
      {createAccess.allowed && (
        <div className="mt-[16px] flex flex-wrap items-center gap-x-[6px] gap-y-[6px]">
          <span className="mr-[4px] font-sans text-xs text-tx-2">{t('first_run.start_from')}</span>
          {STARTING_POINTS.map(([name, preset]) => (
            <Link
              key={preset}
              to={`/synthetics/checks/new?preset=${preset}`}
              className="inline-flex h-[26px] items-center rounded-full bg-bg-2 px-[10px] font-sans text-xs font-medium text-tx-1 outline-none transition-colors hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3"
            >
              {t(`assertions.${name}`)}
            </Link>
          ))}
        </div>
      )}
    </FirstRun>
  );
}
