import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Clock3 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as schedulesApi from '@/api/schedules';
import type { Team } from '@/api/teams';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { DateTimePicker } from '@/shell/DateTimePicker';
import {
  FieldArray,
  FormChecklist,
  FormDrawer,
  FormField,
  FormGroup,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { TimezoneField } from '@/shell/TimezoneField';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import type { UserLite } from '@/shell/useUsers';
import { HolidayField } from '@/shell/weekly-ranges/HolidayField';
import { newRange } from '@/shell/weekly-ranges/model';
import { WeeklyRangesEditor } from '@/shell/weekly-ranges/WeeklyRangesEditor';
import type {
  Rotation,
  Schedule,
  WeeklyRange,
} from '@/types/alerting';

import { DEFAULT_TZ, ScheduleDraftPreview } from './DraftPreview';
import { rotationKindKey } from './model';

const WORKDAY_RANGE: WeeklyRange = {
  weekdayMask: 62,
  start: '00:00',
  end: '24:00',
};

const WEEK_MICROS = 7 * 24 * 3_600_000_000;

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
    activeWindows: [],
    startAt: now.getTime() * 1000,
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
  const manageAccess = useActionAccess({ permission: 'schedules.update' });
  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [teamId, setTeamId] = React.useState('');
  const [timezone, setTimezone] = React.useState(DEFAULT_TZ);
  const [swapNeedsApproval, setSwapNeedsApproval] = React.useState(false);
  const [rotations, setRotations] = React.useState<Rotation[]>([
    newRotation(),
  ]);

  React.useEffect(() => {
    if (!open) return;
    setName(editing?.name ?? '');
    setDescription(editing?.description ?? '');
    setTeamId(editing?.teamId ?? '');
    setTimezone(editing?.timezone ?? DEFAULT_TZ);
    setSwapNeedsApproval(editing?.swapNeedsApproval ?? false);
    setRotations(
      editing?.rotations?.length
        ? editing.rotations.map((rotation) => ({
            ...rotation,
            members: [...rotation.members],
            active_windows: (rotation.activeWindows ?? []).map(
              (range) => ({ ...range }),
            ),
          }))
        : [newRotation()],
    );
  }, [editing, open]);

  const save = useMutation({
    mutationFn: ({ enabled }: { enabled: boolean }) => {
      const payload: schedulesApi.ScheduleInput = {
        name: name.trim(),
        description: description.trim(),
        teamId: teamId || null,
        timezone: timezone.trim() || DEFAULT_TZ,
        enabled,
        rotations: rotations
          .filter((rotation) => rotation.members.length > 0)
          .map((rotation) => ({
            ...rotation,
            name: rotation.name.trim(),
          })),
        overrides: editing?.overrides ?? [],
        swapNeedsApproval: swapNeedsApproval,
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
    orgId: editing?.orgId ?? 'preview',
    name,
    description,
    teamId: teamId || null,
    timezone,
    enabled: true,
    rotations,
    overrides: editing?.overrides ?? [],
    createdBy: editing?.createdBy ?? null,
    updatedBy: editing?.updatedBy ?? null,
    createdAt: editing?.createdAt ?? Date.now() * 1000,
    updatedAt: Date.now() * 1000,
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
                <TimezoneField
                  label={t('schedules.fields.timezone')}
                  value={timezone}
                  onChange={setTimezone}
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
            <label
              data-swap-approval
              className="flex items-start justify-between gap-4 bg-bg-2 px-3 py-2.5"
            >
              <span className="min-w-0">
                <span className="block text-sm text-tx-0">
                  {t('schedules.fields.swap_needs_approval')}
                </span>
                <span className="mt-0.5 block text-xs text-tx-2">
                  {t('schedules.hints.swap_needs_approval')}
                </span>
              </span>
              <Switch
                checked={swapNeedsApproval}
                onCheckedChange={setSwapNeedsApproval}
                aria-label={t('schedules.fields.swap_needs_approval')}
              />
            </label>
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
      ? Math.max(1, rotation.kind.custom.periodSecs / 3600)
      : 24;
  const activeWindows = rotation.activeWindows ?? [];
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
        activeWindows: [WORKDAY_RANGE],
      });
      return;
    }
    if (value === 'custom') {
      onChange({
        ...rotation,
        kind: {
          custom: {
            periodSecs:
              typeof rotation.kind === 'object'
                ? rotation.kind.custom.periodSecs
                : 86_400,
          },
        },
        activeWindows: [],
      });
      return;
    }
    onChange({
      ...rotation,
      kind: value,
      activeWindows: cadence === 'workdays' ? [] : activeWindows,
    });
  };

  return (
    <div className="border-b border-bd-0 pb-5">
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
                  ? 'h-8 rounded bg-bg-1 font-sans text-xs font-strong text-indigo-soft'
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
              value={microsToLocalInput(rotation.startAt)}
              onChange={(value) =>
                onChange({
                  ...rotation,
                  startAt: localInputToMicros(value),
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
                        periodSecs:
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
              <div className="flex h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-3 text-sm text-tx-1">
                <Clock3 className="h-3.5 w-3.5 text-tx-3" />
                {new Date(rotation.startAt / 1000).toLocaleTimeString(
                  undefined,
                  { hour: '2-digit', minute: '2-digit' },
                )}
              </div>
            </FormField>
          )}
        </FormRow>
      </div>

      <div className="mt-4">
        <FormGroup label={t('schedules.fields.members')} required>
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
            <div className="bg-bg-2 px-3 py-4 text-xs text-tx-3">
              {t('schedules.no_users')}
            </div>
          )}
        </FormGroup>
      </div>

      <div className="mt-4 border-t border-bd-0 pt-3" data-rotation-end>
        <label className="flex min-h-9 items-center gap-2 text-xs font-strong text-tx-1">
          <Checkbox
            checked={rotation.endAt != null}
            onCheckedChange={(checked) =>
              onChange({
                ...rotation,
                endAt: checked === true ? rotation.startAt + WEEK_MICROS : null,
              })
            }
          />
          {t('schedules.fields.ends')}
        </label>
        {rotation.endAt != null && (
          <div className="mt-2">
            <FormField label={t('schedules.fields.end_at')}>
              <DateTimePicker
                value={microsToLocalInput(rotation.endAt)}
                onChange={(value) =>
                  onChange({
                    ...rotation,
                    endAt: localInputToMicros(value),
                  })
                }
              />
            </FormField>
          </div>
        )}
      </div>

      {cadence !== 'workdays' && (
        <div className="mt-4 border-t border-bd-0 pt-3">
          <label className="flex min-h-9 items-center gap-2 text-xs font-strong text-tx-1">
            <Checkbox
              checked={activeWindows.length > 0}
              onCheckedChange={(checked) =>
                onChange({
                  ...rotation,
                  activeWindows: checked === true ? [newRange()] : [],
                })
              }
            />
            {t('schedules.fields.restrict_window')}
          </label>
          {activeWindows.length > 0 && (
            <div className="mt-2">
              <WeeklyRangesEditor
                ranges={activeWindows}
                onChange={(ranges) =>
                  onChange({ ...rotation, activeWindows: ranges })
                }
              />
            </div>
          )}
        </div>
      )}

      <div className="mt-4 border-t border-bd-0 pt-3" data-rotation-holidays>
        <HolidayField
          value={rotation.calendar}
          onChange={(calendar) => onChange({ ...rotation, calendar })}
        />
      </div>
    </div>
  );
}
