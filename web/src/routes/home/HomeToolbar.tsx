import {
  BellRing,
  Braces,
  ChevronDown,
  Clock3,
  Database,
  LayoutDashboard,
  Plus,
  RefreshCw,
  Workflow,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

export const HOME_WINDOWS = [
  { seconds: 24 * 60 * 60, labelKey: 'home.toolbar.window_24h' },
  { seconds: 7 * 24 * 60 * 60, labelKey: 'home.toolbar.window_7d' },
] as const;

export function homeWindowFor(seconds: number) {
  return HOME_WINDOWS.find((item) => item.seconds === seconds) ?? HOME_WINDOWS[0];
}

/**
 * Page-level controls: which window the page covers, refresh, and creation.
 * Creation is a quiet button on purpose; the one primary action on Home
 * belongs to the verdict card.
 */
export function HomeToolbar({
  windowSecs,
  onWindowChange,
  onRefresh,
  refreshing,
  updatedAt,
}: {
  windowSecs: number;
  onWindowChange: (seconds: number) => void;
  onRefresh: () => void;
  refreshing: boolean;
  /** Epoch ms of the last successful fetch; 0 when there has not been one. */
  updatedAt: number;
}) {
  const { t, i18n } = useTranslation('onboarding');
  const nav = useNavigate();
  const dashboardCreateAccess = useActionAccess({ permission: 'dashboards.create' });
  const alertCreateAccess = useActionAccess({ permission: 'alerts.manage' });
  const streamCreateAccess = useActionAccess({ permission: 'streams.create' });
  const pipelineCreateAccess = useActionAccess({ permission: 'pipelines.create' });
  const functionCreateAccess = useActionAccess({ permission: 'functions.create' });
  const selectedWindow = homeWindowFor(windowSecs);
  const refreshLabel =
    updatedAt > 0
      ? `${t('home.toolbar.refresh')} · ${t('home.toolbar.updated', {
          time: new Date(updatedAt).toLocaleTimeString(
            i18n.resolvedLanguage ?? i18n.language,
            { hour12: false },
          ),
        })}`
      : t('home.toolbar.refresh');

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <ChromeButton variant="default">
            <Clock3 className="h-3.5 w-3.5" />
            {t(selectedWindow.labelKey)}
            <ChevronDown className="h-3.5 w-3.5 text-tx-3" />
          </ChromeButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-44">
          <DropdownMenuRadioGroup
            value={String(windowSecs)}
            onValueChange={(value) => onWindowChange(Number(value))}
          >
            {HOME_WINDOWS.map((item) => (
              <DropdownMenuRadioItem key={item.seconds} value={String(item.seconds)}>
                {t(item.labelKey)}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
      <ChromeButton
        variant="default"
        className="w-9 justify-center px-0 disabled:border-transparent"
        onClick={onRefresh}
        disabled={refreshing}
        aria-label={t('home.toolbar.refresh')}
        title={refreshLabel}
      >
        <RefreshCw className={cn('h-3.5 w-3.5', refreshing && 'animate-spin')} />
      </ChromeButton>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <ChromeButton variant="default" className="ml-1">
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
          <DropdownMenuSeparator />
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
    </>
  );
}
