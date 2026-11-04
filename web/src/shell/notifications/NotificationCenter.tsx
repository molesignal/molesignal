import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Bell, Check } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, chromeIconButtonSizeClass } from '@/shell/chrome';
import { refreshIncidents } from '@/shell/incident/refresh';
import { SEVERITY_TONE } from '@/shell/incident/tones';
import { cn } from '@/shell/lib/cn';
import { StatusWord, type StatusTone } from '@/shell/StatusWord';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';
import { toast } from '@/shell/ui/sonner';
import { useUiPreference } from '@/shell/uiPreference/useUiPreference';

import {
  countUnread,
  parseReadNotifications,
  settleRead,
  type AppNotification,
  type NotificationCategory,
} from './model';
import { useAppNotifications } from './useAppNotifications';

type Tab = 'all' | NotificationCategory;

function NotificationRow({
  item,
  unread,
  onOpen,
}: {
  item: AppNotification;
  unread: boolean;
  onOpen: (item: AppNotification) => void;
}) {
  const { t } = useTranslation('shell');
  const { t: ta } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const acknowledgeAccess = useActionAccess({ permission: 'alerts.incidents.acknowledge' });

  const acknowledge = useMutation({
    mutationFn: (id: string) => incidentsApi.ack(id),
    onSuccess: () => {
      toast.success(t('notifications.acknowledged'));
      void refreshIncidents(queryClient);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <li className="relative" data-notification={item.category} data-unread={unread || undefined}>
      <button
        type="button"
        onClick={() => onOpen(item)}
        className={cn(
          'flex w-full items-start gap-2 px-3 py-2 text-left hover:bg-bg-3 focus-visible:bg-bg-3 focus-visible:outline-none',
          item.incidentId && acknowledgeAccess.allowed && 'pr-[84px]',
        )}
      >
        <span
          aria-hidden
          className={cn('mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full', unread ? 'bg-indigo' : 'bg-transparent')}
        />
        <span className="min-w-0 flex-1 font-sans text-xs">
          <span className={cn('block truncate text-tx-0', unread && 'font-strong')}>{item.title}</span>
          {(item.severity || item.description) && (
            <span className="mt-0.5 flex min-w-0 items-baseline gap-1.5 text-tx-3">
              {item.severity && (
                <StatusWord tone={SEVERITY_TONE[item.severity] as StatusTone}>
                  {ta(`history.severity.${item.severity}`)}
                </StatusWord>
              )}
              {item.description && <span className="min-w-0 truncate">{item.description}</span>}
            </span>
          )}
        </span>
      </button>
      {item.incidentId && acknowledgeAccess.allowed && (
        <ChromeButton
          size="sm"
          className="absolute right-3 top-2 h-7 px-2"
          disabled={acknowledge.isPending}
          onClick={() => item.incidentId && acknowledge.mutate(item.incidentId)}
          data-testid="notification-ack"
        >
          <Check className="h-3 w-3" />
          {t('notifications.acknowledge')}
        </ChromeButton>
      )}
    </li>
  );
}

/**
 * The bell: what is waiting on the person, wherever it comes from. It rings for
 * what they have not read; what stays listed until it is dealt with (an open
 * incident, a pending approval) stops ringing once read, and is gone once done.
 */
export function NotificationCenter() {
  const { t } = useTranslation('shell');
  const nav = useNavigate();
  const items = useAppNotifications();
  const [open, setOpen] = React.useState(false);
  const [tab, setTab] = React.useState<Tab>('all');

  const kept = useUiPreference('notifications.read', 'read', parseReadNotifications);
  const read = React.useMemo(() => new Set(kept.value?.ids ?? []), [kept.value]);
  const unread = countUnread(items, read);
  const markRead = (ids: string[]) => kept.save({ ids: settleRead(items, read, ids) });

  const shown = tab === 'all' ? items : items.filter((item) => item.category === tab);
  const countOf = (category: NotificationCategory) => items.filter((item) => item.category === category).length;
  const label =
    unread > 0 ? t('notifications.unread_label', { count: unread }) : t('topbar.notifications');

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        title={label}
        aria-label={label}
        data-testid="notification-bell"
        className={cn(
          'relative flex items-center justify-center rounded-md text-tx-2',
          chromeIconButtonSizeClass,
          'hover:bg-bg-3 hover:text-tx-0',
          'focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none',
        )}
      >
        <Bell className="h-4 w-4" />
        {unread > 0 && <span className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-red" />}
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[380px] p-0" data-testid="notification-panel">
        <div className="flex min-h-10 items-center justify-between gap-3 border-b border-bd-0 px-3 py-1.5">
          <span className="font-sans text-xs font-strong text-tx-0">{t('notifications.title')}</span>
          <button
            type="button"
            disabled={unread === 0}
            onClick={() => markRead(items.map((item) => item.id))}
            className="font-sans text-xs text-tx-2 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none disabled:cursor-default disabled:text-[var(--tx-disabled)]"
          >
            {t('notifications.mark_all_read')}
          </button>
        </div>
        <div className="border-b border-bd-0 px-2 py-1.5">
          <ViewTabs<Tab>
            label={t('notifications.tabs_label')}
            value={tab}
            onValueChange={setTab}
            items={[
              { key: 'all', label: t('notifications.tabs.all'), count: items.length },
              { key: 'alerts', label: t('notifications.tabs.alerts'), count: countOf('alerts') },
              { key: 'approvals', label: t('notifications.tabs.approvals'), count: countOf('approvals') },
            ]}
          />
        </div>
        {shown.length === 0 ? (
          <div className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('notifications.empty')}</div>
        ) : (
          <ul className="max-h-80 overflow-auto py-1">
            {shown.map((item) => (
              <NotificationRow
                key={item.id}
                item={item}
                unread={!read.has(item.id)}
                onOpen={(opened) => {
                  markRead([opened.id]);
                  setOpen(false);
                  if (opened.to) nav(opened.to);
                }}
              />
            ))}
          </ul>
        )}
      </PopoverContent>
    </Popover>
  );
}
