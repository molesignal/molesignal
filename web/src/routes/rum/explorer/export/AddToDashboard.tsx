import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { LayoutDashboard } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as dashboardsApi from '@/api/dashboards';
import * as rumApi from '@/api/rum';
import { dashboardDefinitionFromApi } from '@/dashboard-engine/model';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { QueryIconAction } from '@/shell/query/IconAction';
import { QueryToolbarButton, QueryToolbarGroup } from '@/shell/query/Workbench';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';
import type { TimeWindow } from '@/stores/useTimeStore';
import type { Dashboard } from '@/types/dashboard';

import {
  intervalFor,
  intervalVariable,
  MAP_COUNTRIES,
  nearestInterval,
  rumAddedDashboardModel,
  rumDashboardPanel,
  type RumInterval,
  rumNewDashboardModel,
  type RumPanelPlan,
} from './panelModel';

type Target = 'new' | 'existing';

interface RumAddToDashboardProps {
  plan: RumPanelPlan;
  /** The stretch of time the explorer looks at: a new dashboard opens on it. */
  timeWindow: TimeWindow;
  /** The range and the bar the explorer draws, which the interval of a chart over time starts nearest. */
  range: { from: number; to: number };
  bucket: number | null;
  /** Whether the query has been read against the fields of the type: until then there is no panel to make. */
  ready: boolean;
}

/** The dashboards a panel can be added to: not the ones the product installs, which are read only. */
const editable = (dashboards: readonly Dashboard[] | undefined) =>
  (dashboards ?? []).filter((dashboard) => dashboard.model.editable !== false).sort((left, right) => left.title.localeCompare(right.title));

/** The interval a dashboard has, as far as its model can be read here. */
function intervalOf(dashboard: Dashboard | undefined, fixed: RumInterval) {
  if (!dashboard) return null;

  try {
    return intervalFor(dashboardDefinitionFromApi(dashboard), fixed);
  } catch {
    return null;
  }
}

/**
 * Puts what the explorer draws on a dashboard: a new one that holds it alone, or one there is,
 * under what it has. The panel reads the same rows with SQL, the same filters, measures and
 * groups; its range is the dashboard's. The dialog says how the panel draws it, and what a chart
 * over time is cut by.
 */
export function RumAddToDashboard({ plan, timeWindow, range, bucket, ready: known }: RumAddToDashboardProps) {
  const { t } = useTranslation('rum');
  const { t: tCommon } = useTranslation('common');
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const createAccess = useActionAccess({ permission: 'dashboards.create' });
  const editAccess = useActionAccess({ permission: 'dashboards.edit' });
  const [open, setOpen] = React.useState(false);
  const [target, setTarget] = React.useState<Target>('new');
  const [dashboardTitle, setDashboardTitle] = React.useState('');
  const [dashboardId, setDashboardId] = React.useState('');
  const [panelTitle, setPanelTitle] = React.useState('');

  const dashboards = useQuery({
    queryKey: ['dashboards', 'list'],
    queryFn: () => dashboardsApi.list(),
    enabled: open && editAccess.allowed,
  });
  const choices = editable(dashboards.data);
  const chosen = choices.find((dashboard) => dashboard.id === dashboardId);

  // Each time it is opened it starts from what is on screen now, not from what was typed before.
  React.useEffect(() => {
    if (!open) return;

    setTarget(createAccess.allowed ? 'new' : 'existing');
    setDashboardTitle(plan.title);
    setPanelTitle(plan.title);
  }, [createAccess.allowed, open, plan.title]);

  const overTime = plan.chart === 'timeseries';
  const fixed = nearestInterval(range, bucket);
  const addsInterval = overTime && target === 'existing' && Boolean(intervalOf(chosen, fixed)?.adds);

  const save = useMutation({
    mutationFn: async () => {
      if (target === 'new') {
        // A new dashboard has no variable yet: it gets the interval.
        const cut = overTime ? intervalFor(null, fixed) : null;
        const statement = await rumApi.panelStatement({ ...plan.params, ...(cut ? { interval: cut.interval } : {}) });
        const panel = rumDashboardPanel(statement, plan, panelTitle.trim());

        return dashboardsApi.create(
          rumNewDashboardModel(dashboardTitle.trim(), panel, timeWindow, overTime ? intervalVariable(t, fixed) : null),
        );
      }

      // The dashboard as it is now, not as the list had it.
      const dashboard = await dashboardsApi.get(dashboardId);
      const definition = dashboardDefinitionFromApi(dashboard);
      const cut = overTime ? intervalFor(definition, fixed) : null;
      const statement = await rumApi.panelStatement({ ...plan.params, ...(cut ? { interval: cut.interval } : {}) });
      const panel = rumDashboardPanel(statement, plan, panelTitle.trim(), definition.elements);

      return dashboardsApi.update(
        dashboard.id,
        rumAddedDashboardModel(definition, panel, cut?.adds ? intervalVariable(t, fixed) : null),
        dashboard.folderId,
      );
    },
    onSuccess: async (dashboard) => {
      await queryClient.invalidateQueries({ queryKey: ['dashboards'] });
      setOpen(false);
      toast.success(t(target === 'new' ? 'explorer.export.dashboard.created' : 'explorer.export.dashboard.added', { name: dashboard.title }), {
        action: {
          label: t('explorer.export.dashboard.open'),
          onClick: () => navigate(`/dashboards/${dashboard.id}`),
        },
      });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const ready =
    panelTitle.trim() !== '' && (target === 'new' ? dashboardTitle.trim() !== '' && createAccess.allowed : Boolean(chosen));
  const blocked = createAccess.disabled && editAccess.disabled;

  return (
    <>
      <QueryIconAction
        icon={LayoutDashboard}
        label={t('explorer.export.dashboard.action')}
        data-rum-add-to-dashboard=""
        disabled={blocked || !known}
        disabledReason={blocked ? createAccess.reason : undefined}
        onClick={() => setOpen(true)}
      />
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="w-[min(440px,calc(100vw-24px))]" data-rum-dashboard-dialog="">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <LayoutDashboard className="h-4 w-4 text-tx-2" aria-hidden="true" />
              {t('explorer.export.dashboard.title')}
            </DialogTitle>
            <DialogDescription>{t('explorer.export.dashboard.description')}</DialogDescription>
          </DialogHeader>
          <ul data-rum-dashboard-notes="" className="m-0 list-disc space-y-1 pl-4 font-sans text-xs text-tx-2">
            <li data-rum-dashboard-shape={plan.chart}>
              {plan.change === 'list_as_count'
                ? t('explorer.export.dashboard.list_as_count')
                : plan.change === 'map_as_top'
                  ? t('explorer.export.dashboard.map_as_top', { count: MAP_COUNTRIES })
                  : t(`explorer.export.dashboard.shapes.${plan.chart}`)}
            </li>
            {overTime && (
              <li data-rum-dashboard-interval="">
                {addsInterval
                  ? t('explorer.export.dashboard.interval_added')
                  : t('explorer.export.dashboard.interval', { interval: t(`explorer.export.dashboard.intervals.${fixed.key}`) })}
              </li>
            )}
          </ul>
          <form
            className="grid gap-4 py-1"
            onSubmit={(event) => {
              event.preventDefault();
              if (ready && !save.isPending) save.mutate();
            }}
          >
            <QueryToolbarGroup role="group" aria-label={t('explorer.export.dashboard.target')} className="w-fit border-0 bg-[var(--control-surface)]">
              {(['new', 'existing'] as const).map((option) => (
                <QueryToolbarButton
                  key={option}
                  flat
                  active={target === option}
                  aria-pressed={target === option}
                  data-rum-dashboard-target={option}
                  disabled={option === 'new' ? createAccess.disabled : editAccess.disabled}
                  title={option === 'new' ? createAccess.reason : editAccess.reason}
                  onClick={() => setTarget(option)}
                >
                  {t(`explorer.export.dashboard.targets.${option}`)}
                </QueryToolbarButton>
              ))}
            </QueryToolbarGroup>
            {target === 'new' ? (
              <FormField label={t('explorer.export.dashboard.name')}>
                <FormInput value={dashboardTitle} onChange={(event) => setDashboardTitle(event.target.value)} maxLength={120} autoFocus />
              </FormField>
            ) : (
              <FormField
                label={t('explorer.export.dashboard.dashboard')}
                {...(dashboards.isSuccess && choices.length === 0 ? { hint: t('explorer.export.dashboard.none') } : {})}
              >
                <FormSelect
                  ariaLabel={t('explorer.export.dashboard.dashboard')}
                  value={dashboardId}
                  onChange={setDashboardId}
                  options={choices.map((dashboard) => ({ value: dashboard.id, label: dashboard.title }))}
                  placeholder={t('explorer.export.dashboard.choose')}
                  disabled={!dashboards.isSuccess || choices.length === 0}
                />
              </FormField>
            )}
            <FormField label={t('explorer.export.dashboard.panel')}>
              <FormInput value={panelTitle} onChange={(event) => setPanelTitle(event.target.value)} maxLength={120} />
            </FormField>
            {/* A form submits on Enter only through a submit control of its own. */}
            <button type="submit" hidden />
          </form>
          <DialogFooter>
            <ChromeButton onClick={() => setOpen(false)}>{tCommon('actions.cancel')}</ChromeButton>
            <ChromeButton
              variant="primary"
              disabled={!ready || save.isPending}
              onClick={() => save.mutate()}
              data-rum-dashboard-confirm=""
            >
              {save.isPending
                ? t('explorer.export.dashboard.saving')
                : t(target === 'new' ? 'explorer.export.dashboard.create' : 'explorer.export.dashboard.add')}
            </ChromeButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
