import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable, PageHeader } from '@/admin';
import * as holidaysApi from '@/api/holidays';
import type { HolidayCalendar } from '@/api/holidays';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';
import { useHolidayCalendars } from '@/shell/weekly-ranges/HolidayField';

import { CalendarDrawer } from './CalendarDrawer';
import { dayCounts, isoDate, nextHoliday } from './model';
import { SectionBody } from '../_atoms';

/**
 * Holiday calendars: the days off and make-up workdays that weekly hours, rotations, silences and
 * notify policies can count by.
 */
export function HolidayCalendars() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'alerts.calendars.manage' });
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<HolidayCalendar | null>(null);
  const [removing, setRemoving] = React.useState<HolidayCalendar | null>(null);
  const today = isoDate(new Date());

  const calendars = useHolidayCalendars();
  const rows = calendars.data ?? [];
  const state = queryStateFor({ isLoading: calendars.isLoading, isError: calendars.isError, data: rows });
  const pageState = productStateFor(state, {
    error: calendars.error,
    emptyTitle: t('holidays.empty_title'),
    emptyDescription: t('holidays.empty_description'),
  });

  const references = useQuery({
    queryKey: ['holiday-calendars', removing?.id, 'references'],
    queryFn: () => holidaysApi.references(removing?.id ?? ''),
    enabled: removing !== null,
  });
  const used = references.data ?? [];

  const remove = useMutation({
    mutationFn: (id: string) => holidaysApi.remove(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void queryClient.invalidateQueries({ queryKey: ['holiday-calendars'] });
      setRemoving(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <>
      <PageHeader
        title={t('holidays.title')}
        subtitle={t('holidays.subtitle') as string}
        actions={
          <ChromeButton
            variant="primary"
            onClick={() => setCreating(true)}
            disabled={access.disabled}
            disabledReason={access.reason}
            data-testid="holiday-new"
          >
            {t('holidays.new')}
          </ChromeButton>
        }
      />
      <CalendarDrawer
        open={creating || editing !== null}
        editing={editing}
        access={access}
        onClose={() => {
          setCreating(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('holidays.delete_title', { name: removing?.name ?? '' })}
        description={
          used.length > 0
            ? t('holidays.delete_in_use', {
                count: used.length,
                names: used.map((item) => `${t(`holidays.reference_kinds.${item.kind}`)}「${item.name}」`).join('、'),
              })
            : t('holidays.delete_description')
        }
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={access.disabled || used.length > 0 || references.isLoading}
        disabledReason={used.length > 0 ? t('holidays.delete_blocked') : access.reason}
        onConfirm={() => {
          if (removing && access.allowed && used.length === 0) remove.mutate(removing.id);
        }}
      />
      <SectionBody>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(row) => row.id}
            onRowClick={(row) => setEditing(row)}
            columns={[
              { key: 'name', header: t('holidays.columns.name'), cell: (row) => row.name },
              {
                key: 'days',
                header: t('holidays.columns.days'),
                width: 200,
                cell: (row) => {
                  const counts = dayCounts(row.days);
                  return (
                    <span className="tabular-nums text-tx-1">
                      {t('holidays.counts', { holidays: counts.holidays, workdays: counts.workdays })}
                    </span>
                  );
                },
              },
              {
                key: 'next',
                header: t('holidays.columns.next'),
                width: 220,
                cell: (row) => {
                  const next = nextHoliday(row.days, today);
                  return next ? (
                    <span className="tabular-nums text-tx-1">
                      {next.date}
                      {next.name ? ` · ${next.name}` : ''}
                    </span>
                  ) : (
                    <span className="text-tx-3">{t('holidays.none_ahead')}</span>
                  );
                },
              },
              {
                key: 'actions',
                header: '',
                width: 80,
                cell: (row) => (
                  <ChromeButton
                    variant="ghost"
                    disabled={access.disabled}
                    disabledReason={access.reason}
                    onClick={() => setRemoving(row)}
                  >
                    {tc('actions.delete')}
                  </ChromeButton>
                ),
              },
            ]}
          />
        )}
      </SectionBody>
    </>
  );
}
