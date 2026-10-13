import { useMutation, useQueryClient } from '@tanstack/react-query';
import { LayoutDashboard } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as dashboardsApi from '@/api/dashboards';
import type { DashboardPanel } from '@/dashboard-engine/schema';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';
import type { TimeWindow } from '@/stores/useTimeStore';

import { logStatsDashboardModel } from './model';

interface CreateStatsDashboardProps {
  /** The statistics as they are drawn: the panel the dashboard will hold. `null` while there is nothing to save. */
  panel: DashboardPanel | null;
  /** What the dashboard and its panel are called unless the reader says otherwise. */
  suggested: { dashboard: string; panel: string };
  /** The stretch of time the explorer looks at: the dashboard opens on it. */
  timeWindow: TimeWindow;
}

/**
 * Makes a dashboard of the statistics on screen, in one step: a new dashboard
 * that holds them as its only panel. The reader names the two and it is saved;
 * the dashboard is then a click away, and the explorer stays where it was.
 */
export function CreateStatsDashboard({ panel, suggested, timeWindow }: CreateStatsDashboardProps) {
  const { t } = useTranslation('logs');
  const { t: tCommon } = useTranslation('common');
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'dashboards.create' });
  const [open, setOpen] = React.useState(false);
  const [dashboardTitle, setDashboardTitle] = React.useState(suggested.dashboard);
  const [panelTitle, setPanelTitle] = React.useState(suggested.panel);

  // Each time it is opened it starts from what is on screen now, not from what was typed before.
  React.useEffect(() => {
    if (!open) return;

    setDashboardTitle(suggested.dashboard);
    setPanelTitle(suggested.panel);
  }, [open, suggested.dashboard, suggested.panel]);

  const create = useMutation({
    mutationFn: async () => {
      if (!panel) throw new Error('nothing to save');

      return dashboardsApi.create(
        logStatsDashboardModel(dashboardTitle.trim(), { ...panel, title: panelTitle.trim() }, timeWindow),
      );
    },
    onSuccess: async (dashboard) => {
      await queryClient.invalidateQueries({ queryKey: ['dashboards'] });
      setOpen(false);
      toast.success(t('explore.stats.dashboard.created', { name: dashboard.title }), {
        action: {
          label: t('explore.stats.dashboard.open'),
          onClick: () => navigate(`/dashboards/${dashboard.id}`),
        },
      });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const ready = Boolean(panel) && dashboardTitle.trim() !== '' && panelTitle.trim() !== '';

  return (
    <>
      <ChromeButton
        size="sm"
        onClick={() => setOpen(true)}
        disabled={!panel || access.disabled}
        disabledReason={access.disabled ? access.reason : undefined}
        data-log-stats-create-dashboard
      >
        <LayoutDashboard className="h-3.5 w-3.5" aria-hidden="true" />
        {t('explore.stats.dashboard.action')}
      </ChromeButton>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="w-[min(440px,calc(100vw-24px))]">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <LayoutDashboard className="h-4 w-4 text-tx-2" aria-hidden="true" />
              {t('explore.stats.dashboard.title')}
            </DialogTitle>
            <DialogDescription>{t('explore.stats.dashboard.description')}</DialogDescription>
          </DialogHeader>
          <form
            className="grid gap-4 py-1"
            onSubmit={(event) => {
              event.preventDefault();
              if (ready && !create.isPending) create.mutate();
            }}
          >
            <FormField label={t('explore.stats.dashboard.name')}>
              <FormInput
                value={dashboardTitle}
                onChange={(event) => setDashboardTitle(event.target.value)}
                maxLength={120}
                autoFocus
              />
            </FormField>
            <FormField label={t('explore.stats.dashboard.panel')}>
              <FormInput value={panelTitle} onChange={(event) => setPanelTitle(event.target.value)} maxLength={120} />
            </FormField>
            {/* A form submits on Enter only through a submit control of its own. */}
            <button type="submit" hidden />
          </form>
          <DialogFooter>
            <ChromeButton onClick={() => setOpen(false)}>{tCommon('actions.cancel')}</ChromeButton>
            <ChromeButton
              variant="primary"
              disabled={!ready || create.isPending}
              onClick={() => create.mutate()}
              data-log-stats-create-dashboard-confirm
            >
              {create.isPending ? t('explore.stats.dashboard.creating') : tCommon('actions.create')}
            </ChromeButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
