import { X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type * as usersApi from '@/api/users';
import { ChromeButton } from '@/shell/chrome';
import { DateTimePicker, parseLocalDateTime } from '@/shell/DateTimePicker';
import { FormInput } from '@/shell/FormDrawer';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { humanAuditAction } from './humanAction';

export interface AuditFilterValues {
  /** A preset's start (`now-24h`), or `custom` to read `customFrom` and `customTo`. */
  range: string;
  customFrom: string;
  customTo: string;
  actor: string;
  actor_kind: string;
  action: string;
  target_kind: string;
  target_id: string;
}

/** A select's value for "any": it takes no empty string. */
const ALL = '__all__';

export const EMPTY_AUDIT_FILTERS: AuditFilterValues = {
  range: ALL,
  customFrom: '',
  customTo: '',
  actor: '',
  actor_kind: '',
  action: '',
  target_kind: '',
  target_id: '',
};

const RANGES = ['now-1h', 'now-24h', 'now-7d', 'now-30d'] as const;

function localToMicros(value: string): string | undefined {
  const date = parseLocalDateTime(value);
  return date ? String(date.getTime() * 1000) : undefined;
}

/** What the filters ask the server for. An empty one is left out. */
export function auditQueryOf(filters: AuditFilterValues) {
  const custom = filters.range === 'custom';

  return {
    from: custom ? localToMicros(filters.customFrom) : filters.range === ALL ? undefined : filters.range,
    to: custom ? localToMicros(filters.customTo) : undefined,
    actorKind: filters.actor_kind || undefined,
    actor: filters.actor || undefined,
    action: filters.action || undefined,
    targetKind: filters.target_kind || undefined,
    targetId: filters.target_id.trim() || undefined,
  };
}

/**
 * The trail's filters. What can be picked is picked: the actions and kinds come
 * from the trail itself, so nobody has to know `agent.chat.archived` to find it,
 * and a filter takes effect as it is set.
 */
export function AuditFilters({
  value,
  onChange,
  users,
  actions,
  actorKinds,
  targetKinds,
}: {
  value: AuditFilterValues;
  onChange: (next: AuditFilterValues) => void;
  users: readonly usersApi.UserView[];
  actions: readonly string[];
  actorKinds: readonly string[];
  targetKinds: readonly string[];
}) {
  const { t } = useTranslation('settings-admin');
  const set = (patch: Partial<AuditFilterValues>) => onChange({ ...value, ...patch });
  const pick = (field: keyof AuditFilterValues) => (next: string) => set({ [field]: next === ALL ? '' : next });

  const active =
    value.range !== EMPTY_AUDIT_FILTERS.range ||
    Boolean(value.actor || value.actor_kind || value.action || value.target_kind || value.target_id);

  return (
    <div data-audit-filters className="mb-3 flex flex-wrap items-center gap-2">
      <FittedSelect
        maxWidth={260}
        aria-label={t('audit.filters.range')}
        value={value.range}
        onValueChange={(range) => set({ range })}
        options={[
          ...RANGES.map((range) => ({ value: range, label: t(`audit.filters.ranges.${range.replace('now-', '')}`) })),
          { value: ALL, label: t('audit.filters.ranges.all') },
          { value: 'custom', label: t('audit.filters.ranges.custom') },
        ]}
      />
      {value.range === 'custom' && (
        <>
          <DateTimePicker
            aria-label={t('audit.filters.from')}
            value={value.customFrom}
            onChange={(customFrom) => set({ customFrom })}
          />
          <DateTimePicker
            aria-label={t('audit.filters.to')}
            value={value.customTo}
            onChange={(customTo) => set({ customTo })}
          />
        </>
      )}
      <FittedSelect
        maxWidth={260}
        aria-label={t('audit.filters.actor')}
        value={value.actor || ALL}
        onValueChange={pick('actor')}
        options={[
          { value: ALL, label: t('audit.filters.actor_all') },
          ...users.map((user) => ({
            value: user.id,
            // Without `org.members.read` the list has names only.
            label: user.displayName && user.email ? `${user.displayName} (${user.email})` : user.displayName || user.email,
          })),
        ]}
      />
      <FittedSelect
        maxWidth={260}
        aria-label={t('audit.filters.actor_kind')}
        value={value.actor_kind || ALL}
        onValueChange={pick('actor_kind')}
        options={[
          { value: ALL, label: t('audit.filters.actor_kind_all') },
          ...actorKinds.map((kind) => ({ value: kind, label: t(`audit.actor_kinds.${kind}`, { defaultValue: kind }) })),
        ]}
      />
      <FittedSelect
        maxWidth={260}
        aria-label={t('audit.filters.action')}
        value={value.action || ALL}
        onValueChange={pick('action')}
        options={[
          { value: ALL, label: t('audit.filters.action_all') },
          ...actions.map((action) => ({ value: action, label: humanAuditAction(action, t) })),
        ]}
      />
      <FittedSelect
        maxWidth={260}
        aria-label={t('audit.filters.target_kind')}
        value={value.target_kind || ALL}
        onValueChange={pick('target_kind')}
        options={[
          { value: ALL, label: t('audit.filters.target_kind_all') },
          ...targetKinds.map((kind) => ({ value: kind, label: humanAuditAction(kind, t) })),
        ]}
      />
      <FormInput
        aria-label={t('audit.filters.target_id')}
        placeholder={t('audit.filters.target_id')}
        value={value.target_id}
        onChange={(event) => set({ target_id: event.target.value })}
        className="w-40"
      />
      {active && (
        <ChromeButton size="sm" onClick={() => onChange(EMPTY_AUDIT_FILTERS)}>
          <X className="h-3 w-3" />
          {t('audit.filters.reset')}
        </ChromeButton>
      )}
    </div>
  );
}
