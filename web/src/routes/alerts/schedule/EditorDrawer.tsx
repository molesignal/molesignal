import { useMutation, useQueryClient } from '@tanstack/react-query';
import { CalendarDays, Clock3, UsersRound } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as schedulesApi from '@/api/schedules';
import type { Team } from '@/api/teams';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
import { DateTimePicker } from '@/shell/DateTimePicker';
import {
  FieldArray,
  FormChecklist,
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import type { UserLite } from '@/shell/useUsers';
import type {
  ActiveWindow,
  Rotation,
  Schedule,
} from '@/types/alerting';

import {
  buildScheduleTimeline,
  rotationKindKey,
} from './model';
import {
  UserAvatar,
  formatScheduleDay,
  formatScheduleTime,
} from './Ui';

const DEFAULT_TZ = 'UTC';
const WORKDAY_WINDOW: ActiveWindow = {
  weekday_mask: 62,
  hour_start: 0,
  hour_end: 24,
};
const DEFAULT_WINDOW: ActiveWindow = {
  weekday_mask: 62,
  hour_start: 9,
  hour_end: 18,
};

const TIMEZONES = [
  'UTC',
  'Asia/Shanghai',
  'Asia/Hong_Kong',
  'Asia/Tokyo',
  'Europe/London',
  'America/New_York',
  'America/Los_Angeles',
];

export function microsToLocalInput(micros: number): string {
  const date = new Date(micros / 1000);
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function localInputToMicros(value: string): number {
  const millis = new Date(value).getTime();
  return Number.isFinite(millis) ? millis * 1000 : 0;
}

function newRotation(): Rotation {
  const now = new Date();
  now.setMinutes(0, 0, 0);
  now.setHours(now.getHours() + 1);
  return {
    id: globalThis.crypto?.randomUUID?.() ?? `rotation-${Date.now()}`,
    name: 'primary',
    members: [],
    kind: 'daily',
    active_window: null,
    start_at: now.getTime() * 1000,
  };
}

export function ScheduleEditorDrawer({
  open,
  editing,
  users,
  teams,
  onClose,
}: {
  open: boolean;
  editing: Schedule | null;
  users: UserLite[];
  teams: Team[];
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const navigate = useNavigate();
  const manageAccess = useActionAccess({ permission: 'schedules.manage' });
  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [teamId, setTeamId] = React.useState('');
  const [timezone, setTimezone] = React.useState(DEFAULT_TZ);
  const [rotations, setRotations] = React.useState<Rotation[]>([
    newRotation(),
  ]);

  React.useEffect(() => {
    if (!open) return;
    setName(editing?.name ?? '');
    setDescription(editing?.description ?? '');
    setTeamId(editing?.team_id ?? '');
    setTimezone(editing?.timezone ?? DEFAULT_TZ);
    setRotations(
      editing?.rotations?.length
        ? editing.rotations.map((rotation) => ({
            ...rotation,
            members: [...rotation.members],
            active_window: rotation.active_window
              ? { ...rotation.active_window }
              : null,
          }))
        : [newRotation()],
    );
  }, [editing, open]);

  const save = useMutation({
    mutationFn: ({ enabled }: { enabled: boolean }) => {
      const payload: schedulesApi.ScheduleInput = {
        name: name.trim(),
        description: description.trim(),
        team_id: teamId || null,
        timezone: timezone.trim() || DEFAULT_TZ,
        enabled,
        rotations: rotations
          .filter((rotation) => rotation.members.length > 0)
          .map((rotation) => ({
            ...rotation,
            name: rotation.name.trim(),
          })),
        overrides: editing?.overrides ?? [],
      };
      return editing
        ? schedulesApi.update(editing.id, payload)
        : schedulesApi.create(payload);
    },
    onSuccess: (saved) => {
      toast.success(
        editing
          ? t('schedules.toast_updated')
          : saved.enabled
            ? t('schedules.toast_created')
            : t('schedules.toast_draft_saved'),
      );
      void qc.invalidateQueries({ queryKey: ['schedules'] });
      onClose();
      if (!editing) navigate(`/alerts/schedules/${saved.id}`);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const validate = (allowEmpty: boolean) => {
    if (!name.trim()) {
      toast.error(t('schedules.errors.name_required'));
      return false;
    }
    if (
      !allowEmpty
      && rotations.every((rotation) => rotation.members.length === 0)
    ) {
      toast.error(t('schedules.errors.members_required'));
      return false;
    }
    return true;
  };

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!manageAccess.allowed) return;
    if (!validate(false)) return;
    save.mutate({ enabled: editing?.enabled ?? true });
  };

  const saveDraft = () => {
    if (!manageAccess.allowed) return;
    if (!validate(true)) return;
    save.mutate({ enabled: false });
  };

  const previewSchedule: Schedule = {
    id: editing?.id ?? 'preview',
    org_id: editing?.org_id ?? 'preview',
    name,
    description,
    team_id: teamId || null,
    timezone,
    enabled: true,
    rotations,
    overrides: editing?.overrides ?? [],
    created_by: editing?.created_by ?? null,
    updated_by: editing?.updated_by ?? null,
    created_at: editing?.created_at ?? Date.now() * 1000,
    updated_at: Date.now() * 1000,
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={
        editing
          ? t('schedules.edit_title')
          : t('schedules.drawer_title')
      }
      subtitle={t('schedules.drawer_subtitle')}
      width="76vw"
      bodyClassName="overflow-hidden p-0"
      footer={
        <>
          <ChromeButton type="button" onClick={onClose}>
            {tc('actions.cancel')}
          </ChromeButton>
          {!editing && (
            <ChromeButton
              type="button"
              disabled={save.isPending || manageAccess.disabled}
              disabledReason={!save.isPending ? manageAccess.reason : undefined}
              onClick={saveDraft}
            >
              {t('schedules.save_draft')}
            </ChromeButton>
          )}
          <ChromeButton
            type="submit"
            form="schedule-editor-form"
            variant="primary"
            disabled={save.isPending || manageAccess.disabled}
            disabledReason={!save.isPending ? manageAccess.reason : undefined}
          >
            {save.isPending
              ? tc('status.saving')
              : editing
                ? t('schedules.save')
                : t('schedules.create')}
          </ChromeButton>
        </>
      }
    >
      <form
        id="schedule-editor-form"
        onSubmit={submit}
        className="grid h-full min-h-0 grid-cols-1 overflow-auto xl:grid-cols-[minmax(0,1.35fr)_minmax(340px,.65fr)] xl:overflow-hidden"
      >
        <fieldset
          disabled={manageAccess.disabled}
          aria-disabled={manageAccess.disabled || undefined}
          className="contents"
        >
        <div className="overflow-auto px-6 py-5">
          <FormSection title={t('schedules.sections.identity')}>
            <FormField label={t('schedules.fields.name')} required>
              <FormInput
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder={t('schedules.placeholders.name')}
                required
              />
            </FormField>
            <FormField label={t('schedules.fields.description')}>
              <FormTextarea
                value={description}
                onChange={(event) => setDescription(event.target.value)}
                placeholder={t('schedules.placeholders.description')}
                className="min-h-20"
              />
            </FormField>
            <FormRow>
              <FormField label={t('schedules.fields.timezone')}>
                <FormSelect
                  value={timezone}
                  onChange={setTimezone}
                  options={TIMEZONES}
                />
              </FormField>
              <FormField label={t('schedules.fields.team')}>
                <FormSelect
                  value={teamId}
                  onChange={setTeamId}
                  options={[
                    {
                      value: '',
                      label: t('schedules.team_unassigned'),
                    },
                    ...teams.map((team) => ({
                      value: team.id,
                      label: team.name,
                    })),
                  ]}
                />
              </FormField>
            </FormRow>
          </FormSection>

          <FormSection
            title={t('schedules.sections.rotations')}
            description={t(
              'schedules.sections.rotations_description',
            )}
            className="mb-0"
          >
            <FieldArray<Rotation>
              items={rotations}
              onChange={setRotations}
              minItems={1}
              addLabel={t('schedules.add_rotation')}
              removeLabel={t('schedules.remove_rotation')}
              newItem={newRotation}
              renderItem={(rotation, index, setRotation) => (
                <RotationEditor
                  rotation={rotation}
                  index={index}
                  users={users}
                  onChange={setRotation}
                />
              )}
            />
          </FormSection>
        </div>

        <ScheduleDraftPreview
          schedule={previewSchedule}
          users={users}
          locale={i18n.language}
        />
        </fieldset>
      </form>
    </FormDrawer>
  );
}

function RotationEditor({
  rotation,
  index,
  users,
  onChange,
}: {
  rotation: Rotation;
  index: number;
  users: UserLite[];
  onChange: (rotation: Rotation) => void;
}) {
  const { t } = useTranslation('alerts');
  const cadence = rotationKindKey(rotation);
  const customHours =
    typeof rotation.kind === 'object'
      ? Math.max(1, rotation.kind.custom.period_secs / 3600)
      : 24;
  const activeWindow = rotation.active_window ?? null;
  const cadenceOptions = [
    'daily',
    'weekly',
    'workdays',
    'custom',
  ] as const;

  const setCadence = (value: (typeof cadenceOptions)[number]) => {
    if (value === 'workdays') {
      onChange({
        ...rotation,
        kind: 'daily',
        active_window: WORKDAY_WINDOW,
      });
      return;
    }
    if (value === 'custom') {
      onChange({
        ...rotation,
        kind: {
          custom: {
            period_secs:
              typeof rotation.kind === 'object'
                ? rotation.kind.custom.period_secs
                : 86_400,
          },
        },
        active_window: null,
      });
      return;
    }
    onChange({
      ...rotation,
      kind: value,
      active_window:
        cadence === 'workdays'
          ? null
          : rotation.active_window ?? null,
    });
  };

  return (
    <div className="rounded-lg border border-bd-0 bg-bg-1 p-4">
      <div className="mb-4 flex items-center gap-2">
        <span className="grid h-6 w-6 shrink-0 place-items-center rounded-full bg-indigo-dim font-mono text-xs font-strong text-indigo-soft">
          {index + 1}
        </span>
        <FormInput
          value={rotation.name}
          onChange={(event) =>
            onChange({ ...rotation, name: event.target.value })
          }
          placeholder={t('schedules.rotation_name_placeholder')}
          className="flex-1"
        />
      </div>

      <FormField label={t('schedules.fields.cadence')}>
        <div className="grid grid-cols-2 gap-1 rounded-md border border-bd-0 bg-bg-2 p-1 sm:grid-cols-4">
          {cadenceOptions.map((option) => (
            <button
              key={option}
              type="button"
              aria-pressed={cadence === option}
              onClick={() => setCadence(option)}
              className={
                cadence === option
                  ? 'h-8 rounded bg-bg-1 font-sans text-xs font-strong text-indigo-soft shadow-sm'
                  : 'h-8 rounded font-sans text-xs font-strong text-tx-3 hover:text-tx-1'
              }
            >
              {t(`schedules.rotation_kinds.${option}`)}
            </button>
          ))}
        </div>
      </FormField>

      <div className="mt-4">
        <FormRow>
          <FormField label={t('schedules.fields.start_at')}>
            <DateTimePicker
              value={microsToLocalInput(rotation.start_at)}
              onChange={(value) =>
                onChange({
                  ...rotation,
                  start_at: localInputToMicros(value),
                })
              }
            />
          </FormField>
          {cadence === 'custom' ? (
            <FormField label={t('schedules.fields.period_hours')}>
              <FormInput
                type="number"
                min={1}
                value={String(customHours)}
                onChange={(event) =>
                  onChange({
                    ...rotation,
                    kind: {
                      custom: {
                        period_secs:
                          Math.max(
                            1,
                            Math.floor(Number(event.target.value)) || 1,
                          ) * 3600,
                      },
                    },
                  })
                }
              />
            </FormField>
          ) : (
            <FormField label={t('schedules.fields.handoff_time')}>
              <div className="flex h-9 items-center gap-2 rounded-md border border-bd-1 bg-bg-2 px-3 text-sm text-tx-1">
                <Clock3 className="h-3.5 w-3.5 text-tx-3" />
                {new Date(rotation.start_at / 1000).toLocaleTimeString(
                  undefined,
                  { hour: '2-digit', minute: '2-digit' },
                )}
              </div>
            </FormField>
          )}
        </FormRow>
      </div>

      <div className="mt-4">
        <FormField label={t('schedules.fields.members')} required>
          {users.length > 0 ? (
            <FormChecklist
              options={users.map((user) => ({
                value: user.id,
                label: user.name,
              }))}
              selected={rotation.members}
              onChange={(members) => onChange({ ...rotation, members })}
            />
          ) : (
            <div className="rounded-md border border-dashed border-bd-1 px-3 py-4 text-xs text-tx-3">
              {t('schedules.no_users')}
            </div>
          )}
        </FormField>
      </div>

      {cadence !== 'workdays' && (
        <div className="mt-4 border-t border-bd-0 pt-3">
          <label className="flex min-h-9 items-center gap-2 text-xs font-strong text-tx-1">
            <Checkbox
              checked={activeWindow !== null}
              onCheckedChange={(checked) =>
                onChange({
                  ...rotation,
                  active_window:
                    checked === true ? DEFAULT_WINDOW : null,
                })
              }
            />
            {t('schedules.fields.restrict_window')}
          </label>
          {activeWindow && (
            <ActiveWindowEditor
              window={activeWindow}
              onChange={(window) =>
                onChange({ ...rotation, active_window: window })
              }
            />
          )}
        </div>
      )}
    </div>
  );
}

function ActiveWindowEditor({
  window,
  onChange,
}: {
  window: ActiveWindow;
  onChange: (window: ActiveWindow) => void;
}) {
  const { t } = useTranslation('alerts');
  return (
    <div className="mt-2 rounded-md border border-bd-0 bg-bg-2 p-3">
      <div className="flex flex-wrap gap-1">
        {Array.from({ length: 7 }, (_, day) => {
          const enabled = (window.weekday_mask & (1 << day)) !== 0;
          return (
            <button
              key={day}
              type="button"
              onClick={() =>
                onChange({
                  ...window,
                  weekday_mask: window.weekday_mask ^ (1 << day),
                })
              }
              className={
                enabled
                  ? 'h-7 rounded border border-indigo/40 bg-indigo-dim px-2 text-xs font-strong text-indigo-soft'
                  : 'h-7 rounded border border-bd-0 bg-bg-1 px-2 text-xs text-tx-3'
              }
            >
              {new Intl.DateTimeFormat(undefined, {
                weekday: 'short',
              }).format(new Date(Date.UTC(2023, 0, 1 + day)))}
            </button>
          );
        })}
      </div>
      <FormRow className="mt-3">
        <FormField label={t('schedules.fields.hour_start')}>
          <FormInput
            type="number"
            min={0}
            max={24}
            value={String(window.hour_start)}
            onChange={(event) =>
              onChange({
                ...window,
                hour_start: clampHour(event.target.value),
              })
            }
          />
        </FormField>
        <FormField label={t('schedules.fields.hour_end')}>
          <FormInput
            type="number"
            min={0}
            max={24}
            value={String(window.hour_end)}
            onChange={(event) =>
              onChange({
                ...window,
                hour_end: clampHour(event.target.value),
              })
            }
          />
        </FormField>
      </FormRow>
    </div>
  );
}

function ScheduleDraftPreview({
  schedule,
  users,
  locale,
}: {
  schedule: Schedule;
  users: UserLite[];
  locale: string;
}) {
  const { t } = useTranslation('alerts');
  const byId = React.useMemo(
    () => new Map(users.map((user) => [user.id, user])),
    [users],
  );
  const [now] = React.useState(() => Date.now() * 1000);
  const segments = React.useMemo(
    () => buildScheduleTimeline(schedule, now, 7).slice(0, 7),
    [now, schedule],
  );

  return (
    <aside className="border-t border-bd-0 bg-bg-2 px-5 py-5 xl:overflow-auto xl:border-l xl:border-t-0">
      <div className="flex items-center gap-2">
        <CalendarDays className="h-4 w-4 text-indigo-soft" />
        <h3 className="type-section-title font-sans font-display text-tx-0">
          {t('schedules.preview.title')}
        </h3>
      </div>
      <p className="mt-1 text-xs leading-relaxed text-tx-3">
        {t('schedules.preview.description')}
      </p>

      <div className="mt-4 flex flex-col gap-2">
        {segments.length === 0 ? (
          <div className="rounded-md border border-dashed border-bd-1 bg-bg-1 px-4 py-8 text-center text-xs text-tx-3">
            {t('schedules.preview.empty')}
          </div>
        ) : (
          segments.map((segment, index) => {
            const user = segment.userId
              ? byId.get(segment.userId)
              : undefined;
            return (
              <div
                key={segment.id}
                className="flex items-center gap-3 rounded-md border border-bd-0 bg-bg-1 px-3 py-2.5"
              >
                <span className="w-20 shrink-0">
                  <span className="block text-xs font-strong text-tx-1">
                    {formatScheduleDay(
                      segment.startAt,
                      locale,
                      schedule.timezone,
                    )}
                  </span>
                  <span className="mt-0.5 block font-mono text-type-micro text-tx-3">
                    {formatScheduleTime(
                      segment.startAt,
                      locale,
                      schedule.timezone,
                    )}
                  </span>
                </span>
                <UserAvatar user={user} size="sm" />
                <span className="min-w-0 flex-1 truncate text-xs font-strong text-tx-0">
                  {user?.name ?? t('schedules.nobody_on_call')}
                </span>
                {segment.source === 'override' && (
                  <Pill tone="orange">
                    {t('schedules.override_badge')}
                  </Pill>
                )}
                {segment.source === 'gap' && (
                  <Pill tone="red">{t('schedules.status.gap')}</Pill>
                )}
                {index < segments.length - 1 && (
                  <span className="sr-only">
                    {t('schedules.preview.then')}
                  </span>
                )}
              </div>
            );
          })
        )}
      </div>

      <div className="mt-4 grid grid-cols-2 gap-2">
        <div className="rounded-md border border-bd-0 bg-bg-1 p-3">
          <UsersRound className="h-4 w-4 text-blue-soft" />
          <div className="mt-2 text-xs text-tx-3">
            {t('schedules.preview.member_count')}
          </div>
          <div className="mt-0.5 font-sans text-lg font-display text-tx-0">
            {
              new Set(
                schedule.rotations.flatMap(
                  (rotation) => rotation.members,
                ),
              ).size
            }
          </div>
        </div>
        <div className="rounded-md border border-bd-0 bg-bg-1 p-3">
          <Clock3 className="h-4 w-4 text-green-soft" />
          <div className="mt-2 text-xs text-tx-3">
            {t('schedules.preview.timezone')}
          </div>
          <div className="mt-0.5 truncate font-mono text-sm font-strong text-tx-0">
            {schedule.timezone || DEFAULT_TZ}
          </div>
        </div>
      </div>
    </aside>
  );
}

function clampHour(value: string): number {
  const number = Math.floor(Number(value));
  if (!Number.isFinite(number)) return 0;
  return Math.min(24, Math.max(0, number));
}
