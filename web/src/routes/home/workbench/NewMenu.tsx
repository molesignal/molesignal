import { BellRing, Braces, ChevronDown, Database, LayoutDashboard, Plus, Workflow } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

/** What can be made from the workbench, each where it is made. */
export function NewMenu() {
  const { t } = useTranslation('onboarding');
  const nav = useNavigate();
  const dashboardCreateAccess = useActionAccess({ permission: 'dashboards.create' });
  const alertCreateAccess = useActionAccess({ permission: 'alerts.rules.manage' });
  const streamCreateAccess = useActionAccess({ permission: 'streams.create' });
  const pipelineCreateAccess = useActionAccess({ permission: 'pipelines.create' });
  const functionCreateAccess = useActionAccess({ permission: 'functions.create' });
  const creatable = [
    dashboardCreateAccess,
    alertCreateAccess,
    streamCreateAccess,
    pipelineCreateAccess,
    functionCreateAccess,
  ].some((access) => access.allowed);

  // Whoever can make none of these is not shown a menu of greyed-out items.
  if (!creatable) return null;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <ChromeButton variant="default" data-testid="workbench-new">
          <Plus className="h-3.5 w-3.5" />
          {t('home.toolbar.new')}
          <ChevronDown className="h-3.5 w-3.5 text-tx-3" />
        </ChromeButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-48">
        <DropdownMenuItem
          disabled={dashboardCreateAccess.disabled}
          disabledReason={dashboardCreateAccess.reason}
          onSelect={() => nav('/dashboards/new/edit')}
        >
          <LayoutDashboard className="h-4 w-4 text-tx-2" />
          {t('home.toolbar.new_dashboard')}
        </DropdownMenuItem>
        <DropdownMenuItem
          disabled={alertCreateAccess.disabled}
          disabledReason={alertCreateAccess.reason}
          onSelect={() => nav('/alerts/rules/new')}
        >
          <BellRing className="h-4 w-4 text-tx-2" />
          {t('home.toolbar.new_alert')}
        </DropdownMenuItem>
        <DropdownMenuItem
          disabled={streamCreateAccess.disabled}
          disabledReason={streamCreateAccess.reason}
          onSelect={() => nav('/streams?create=1')}
        >
          <Database className="h-4 w-4 text-tx-2" />
          {t('home.toolbar.new_stream')}
        </DropdownMenuItem>
        <DropdownMenuItem
          disabled={pipelineCreateAccess.disabled}
          disabledReason={pipelineCreateAccess.reason}
          onSelect={() => nav('/pipelines/new')}
        >
          <Workflow className="h-4 w-4 text-tx-2" />
          {t('home.toolbar.new_pipeline')}
        </DropdownMenuItem>
        <DropdownMenuItem
          disabled={functionCreateAccess.disabled}
          disabledReason={functionCreateAccess.reason}
          onSelect={() => nav('/functions/new')}
        >
          <Braces className="h-4 w-4 text-tx-2" />
          {t('home.toolbar.new_function')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
