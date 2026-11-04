import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Trash2, Upload } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as holidaysApi from '@/api/holidays';
import type { DayKind, HolidayCalendar, HolidayDay } from '@/api/holidays';
import { toApiError } from '@/lib/http';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { DateTimePicker } from '@/shell/DateTimePicker';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { daysOfYear, isoDate, mergeDays, sortedDays, yearsOf } from './model';

const KINDS: readonly DayKind[] = ['holiday', 'workday'];

/** A holiday calendar: its name, and its days off and make-up workdays, a year at a time. */
export function CalendarDrawer({
  open,
  editing,
  access,
  onClose,
}: {
  open: boolean;
  editing: HolidayCalendar | null;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('settings-admin');
  const queryClient = useQueryClient();
  const today = isoDate(new Date());
  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [days, setDays] = React.useState<HolidayDay[]>([]);
  const [year, setYear] = React.useState(today.slice(0, 4));
  const [adding, setAdding] = React.useState<HolidayDay>({ date: today, kind: 'holiday', name: '' });
  const file = React.useRef<HTMLInputElement>(null);

  React.useEffect(() => {
    if (!open) return;
    setName(editing?.name ?? '');
    setDescription(editing?.description ?? '');
    setDays(sortedDays(editing?.days ?? []));
    setYear(today.slice(0, 4));
    setAdding({ date: today, kind: 'holiday', name: '' });
    // `today` only changes at midnight; the drawer starts over when it opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, editing]);

  const save = useMutation({
    mutationFn: () => {
      const input = { name: name.trim(), description: description.trim(), days };
      return editing ? holidaysApi.update(editing.id, input) : holidaysApi.create(input);
    },
    onSuccess: () => {
      toast.success(t(editing ? 'holidays.updated' : 'holidays.created'));
      void queryClient.invalidateQueries({ queryKey: ['holiday-calendars'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const read = useMutation({
    mutationFn: async (picked: File) => holidaysApi.parse(await picked.text()),
    onSuccess: (incoming) => {
      setDays((current) => mergeDays(current, incoming));
      const first = incoming[0]?.date.slice(0, 4);
      if (first) setYear(first);
      toast.success(t('holidays.imported', { count: incoming.length }));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const shown = daysOfYear(days, year);
  const change = (date: string, next: Partial<HolidayDay>) =>
    setDays((current) => current.map((day) => (day.date === date ? { ...day, ...next } : day)));
  const invalid = !name.trim();

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t(editing ? 'holidays.edit_title' : 'holidays.new_title')}
      subtitle={t('holidays.drawer_subtitle')}
      footer={
        <FormSubmitFooter
          formId="holiday-calendar-form"
          busy={save.isPending}
          disabled={access.disabled}
          disabledReason={access.reason}
          invalid={invalid}
          onCancel={onClose}
          submitLabel={t(editing ? 'holidays.save' : 'holidays.create')}
        />
      }
    >
      <form
        id="holiday-calendar-form"
        data-holiday-calendar
        onSubmit={(event) => {
          event.preventDefault();
          if (!access.allowed || invalid) return;
          save.mutate();
        }}
      >
        <FormSection title={t('holidays.sections.calendar')}>
          <FormField label={t('holidays.fields.name')} required>
            <FormInput value={name} onChange={(event) => setName(event.target.value)} required />
          </FormField>
          <FormField label={t('holidays.fields.description')}>
            <FormInput value={description} onChange={(event) => setDescription(event.target.value)} />
          </FormField>
        </FormSection>
        <FormSection title={t('holidays.sections.days')} className="mb-0">
          <div className="flex flex-wrap items-end gap-2">
            <FormField label={t('holidays.fields.year')}>
              <FormSelect
                ariaLabel={t('holidays.fields.year')}
                value={year}
                // Radix reports '' when a year and its option arrive in one render (an import, the
                // drawer opening); nobody picks no year.
                onChange={(next) => {
                  if (next) setYear(next);
                }}
                options={yearsOf(days, today).map((value) => ({ value, label: value }))}
              />
            </FormField>
            <ChromeButton
              type="button"
              onClick={() => file.current?.click()}
              disabled={read.isPending}
              data-testid="holiday-import"
            >
              <Upload className="h-3.5 w-3.5" />
              {t('holidays.import')}
            </ChromeButton>
            <input
              ref={file}
              type="file"
              accept=".ics,text/calendar"
              className="hidden"
              aria-label={t('holidays.import')}
              onChange={(event) => {
                const picked = event.target.files?.[0];
                event.target.value = '';
                if (picked) read.mutate(picked);
              }}
            />
          </div>
          <p className="mt-1 text-xs text-tx-3">{t('holidays.import_hint')}</p>

          <ul className="mt-3 flex flex-col gap-1" data-holiday-days={year}>
            {shown.length === 0 && <li className="py-2 text-xs text-tx-3">{t('holidays.no_days', { year })}</li>}
            {shown.map((day) => (
              <li key={day.date} className="grid grid-cols-[110px_120px_minmax(0,1fr)_32px] items-center gap-2" data-holiday-day={day.date}>
                <span className="tabular-nums text-tx-1">{day.date}</span>
                <FormSelect
                  ariaLabel={t('holidays.fields.kind')}
                  value={day.kind}
                  onChange={(kind) => change(day.date, { kind: kind as DayKind })}
                  options={KINDS.map((kind) => ({ value: kind, label: t(`holidays.kinds.${kind}`) }))}
                />
                <FormInput
                  aria-label={t('holidays.fields.day_name')}
                  value={day.name}
                  onChange={(event) => change(day.date, { name: event.target.value })}
                />
                <IconButton
                  aria-label={t('holidays.remove_day', { date: day.date })}
                  title={t('holidays.remove_day', { date: day.date })}
                  className="enabled:hover:text-red-soft"
                  onClick={() => setDays((current) => current.filter((item) => item.date !== day.date))}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </IconButton>
              </li>
            ))}
          </ul>

          <FormRow>
            <FormField label={t('holidays.fields.date')}>
              <DateTimePicker
                dateOnly
                aria-label={t('holidays.fields.date')}
                value={adding.date}
                onChange={(date) => setAdding({ ...adding, date })}
              />
            </FormField>
            <FormField label={t('holidays.fields.kind')}>
              <FormSelect
                ariaLabel={t('holidays.fields.kind')}
                value={adding.kind}
                onChange={(kind) => setAdding({ ...adding, kind: kind as DayKind })}
                options={KINDS.map((kind) => ({ value: kind, label: t(`holidays.kinds.${kind}`) }))}
              />
            </FormField>
          </FormRow>
          <div className="flex items-end gap-2">
            <FormField label={t('holidays.fields.day_name')} className="flex-1">
              <FormInput value={adding.name} onChange={(event) => setAdding({ ...adding, name: event.target.value })} />
            </FormField>
            <ChromeButton
              type="button"
              disabled={!/^\d{4}-\d{2}-\d{2}$/.test(adding.date)}
              data-testid="holiday-add-day"
              onClick={() => {
                setDays((current) => mergeDays(current, [{ ...adding, name: adding.name.trim() }]));
                setYear(adding.date.slice(0, 4));
                setAdding({ ...adding, name: '' });
              }}
            >
              {t('holidays.add_day')}
            </ChromeButton>
          </div>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
