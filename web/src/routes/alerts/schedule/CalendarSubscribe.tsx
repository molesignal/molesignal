import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CalendarPlus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as schedulesApi from '@/api/schedules';
import type { IssuedCalendarFeed } from '@/api/schedules';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { CopyableValue } from '@/routes/settings/_atoms';
import { ChromeButton } from '@/shell/chrome';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';

const FEEDS_KEY = ['schedule-calendar-feeds'];

/**
 * A calendar subscription: one schedule's shifts, or the reader's own in every schedule when no
 * schedule is given. The address is shown once, right after it is made or reset.
 */
export function CalendarSubscribeDialog({
  open,
  schedule,
  onClose,
}: {
  open: boolean;
  schedule?: { id: string; name: string } | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const [issued, setIssued] = React.useState<IssuedCalendarFeed | null>(null);
  const scheduleId = schedule?.id ?? null;

  React.useEffect(() => {
    if (!open) setIssued(null);
  }, [open]);

  const feeds = useQuery({
    queryKey: FEEDS_KEY,
    queryFn: schedulesApi.calendarFeeds,
    enabled: open,
    retry: false,
  });
  const existing = (feeds.data ?? []).find((feed) => (feed.scheduleId ?? null) === scheduleId);

  const create = useMutation({
    mutationFn: () => schedulesApi.createCalendarFeed(scheduleId ?? undefined),
    onSuccess: (data) => {
      setIssued(data);
      void queryClient.invalidateQueries({ queryKey: FEEDS_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const revoke = useMutation({
    mutationFn: (id: string) => schedulesApi.deleteCalendarFeed(id),
    onSuccess: () => {
      toast.success(t('schedules.calendar.revoked'));
      setIssued(null);
      void queryClient.invalidateQueries({ queryKey: FEEDS_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const address = issued ? issued.url ?? `${window.location.origin}${issued.path}` : '';
  const busy = create.isPending || revoke.isPending;

  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      {/* The address sits in a well like on the page; the dialog's gray would hide it. */}
      <DialogContent className="bg-[var(--functional-surface)]">
        <DialogHeader>
          <DialogTitle>
            {schedule
              ? t('schedules.calendar.title_schedule', { name: schedule.name })
              : t('schedules.calendar.title_mine')}
          </DialogTitle>
          <DialogDescription>
            {schedule ? t('schedules.calendar.description_schedule') : t('schedules.calendar.description_mine')}
          </DialogDescription>
        </DialogHeader>

        <div data-calendar-subscription className="flex min-w-0 flex-col gap-3 font-sans text-sm">
          {issued ? (
            <>
              <div className="flex min-w-0 flex-col gap-1.5">
                <span className="text-xs font-strong text-tx-1">{t('schedules.calendar.address')}</span>
                <CopyableValue value={address} copyLabel={t('schedules.calendar.copy')} copiedLabel={tc('actions.copied')} />
              </div>
              <p className="text-xs text-tx-2">{t('schedules.calendar.shown_once')}</p>
              {!issued.url && <p className="text-xs text-tx-2">{t('schedules.calendar.no_external_url')}</p>}
              <a
                href={address.replace(/^https?:\/\//, 'webcal://')}
                className="inline-flex w-fit items-center gap-1.5 text-xs font-strong text-indigo-soft underline-offset-2 hover:underline"
              >
                <CalendarPlus className="h-3.5 w-3.5" />
                {t('schedules.calendar.open_app')}
              </a>
            </>
          ) : feeds.isLoading ? (
            <p className="text-xs text-tx-3">{t('schedules.calendar.loading')}</p>
          ) : existing ? (
            <div data-calendar-existing className="flex min-w-0 flex-col gap-1 text-xs text-tx-2">
              <span className="text-sm text-tx-0">
                {t('schedules.calendar.subscribed', { created: formatMicrosActive(existing.createdAt, false) })}
              </span>
              <span>
                {existing.lastUsedAt
                  ? t('schedules.calendar.last_used', { value: formatMicrosActive(existing.lastUsedAt, false) })
                  : t('schedules.calendar.never_used')}
              </span>
              <span>{t('schedules.calendar.reset_hint')}</span>
            </div>
          ) : null}
        </div>

        <DialogFooter>
          {issued ? (
            <ChromeButton variant="primary" onClick={onClose}>
              {tc('actions.close')}
            </ChromeButton>
          ) : existing ? (
            <>
              <ChromeButton disabled={busy} onClick={() => revoke.mutate(existing.id)}>
                {t('schedules.calendar.revoke')}
              </ChromeButton>
              <ChromeButton variant="primary" disabled={busy} onClick={() => create.mutate()}>
                {t('schedules.calendar.reset')}
              </ChromeButton>
            </>
          ) : (
            <ChromeButton variant="primary" disabled={busy || feeds.isLoading} onClick={() => create.mutate()}>
              {t('schedules.calendar.create')}
            </ChromeButton>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
