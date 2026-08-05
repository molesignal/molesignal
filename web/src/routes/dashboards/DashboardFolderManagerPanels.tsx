import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  LayoutDashboard,
  Plus,
  Trash2,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { ChromeButton, Pill } from '@/shell/chrome';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';

import {
  DEFAULT_FOLDER,
  type FolderSummary,
} from './folderModel';

export function FolderTreeRow({
  active,
  count,
  depth,
  expanded = false,
  expandable = false,
  label,
  managed = true,
  root = false,
  onClick,
  onToggle,
}: {
  active: boolean;
  count: number;
  depth: number;
  expanded?: boolean;
  expandable?: boolean;
  label: string;
  managed?: boolean;
  root?: boolean;
  onClick: () => void;
  onToggle?: () => void;
}) {
  const { t } = useTranslation('dashboards');
  const Icon = root ? LayoutDashboard : active || expanded ? FolderOpen : Folder;
  return (
    <div
      className={`flex h-9 w-full items-center rounded-md pr-2 font-sans text-xs ${
        active ? 'bg-indigo-dim text-indigo-soft' : 'text-tx-1 hover:bg-bg-2'
      }`}
      style={{ paddingInlineStart: `${10 + depth * 16}px` }}
    >
      {expandable ? (
        <button
          type="button"
          aria-expanded={expanded}
          aria-label={t(
            expanded ? 'folders.collapse_folder' : 'folders.expand_folder',
            { name: label },
          )}
          onClick={onToggle}
          className="grid h-7 w-7 shrink-0 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0"
        >
          {expanded ? (
            <ChevronDown className="h-3.5 w-3.5" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5" />
          )}
        </button>
      ) : (
        <span aria-hidden="true" className="h-7 w-7 shrink-0" />
      )}
      <button
        type="button"
        aria-current={active ? 'page' : undefined}
        onClick={onClick}
        className="flex h-full min-w-0 flex-1 items-center gap-2 text-left"
      >
        <Icon className="h-4 w-4 shrink-0" />
        <span className="min-w-0 flex-1 truncate font-strong">{label}</span>
        {!managed && !root && (
          <Pill tone="dim">{t('folders.read_only')}</Pill>
        )}
        <span className="tabular-nums text-tx-3">{count}</span>
      </button>
    </div>
  );
}

export function FolderCreateForm({
  name,
  parent,
  parentOptions,
  pending,
  disabled,
  disabledReason,
  onNameChange,
  onParentChange,
  onCancel,
  onSubmit,
}: {
  name: string;
  parent: string;
  parentOptions: Array<{ value: string; label: string }>;
  pending: boolean;
  disabled: boolean;
  disabledReason: string | undefined;
  onNameChange: (value: string) => void;
  onParentChange: (value: string) => void;
  onCancel: () => void;
  onSubmit: () => void;
}) {
  const { t } = useTranslation('dashboards');
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit();
      }}
      className="mx-auto max-w-xl"
    >
      <div className="mb-6">
        <h3 className="font-sans text-lg font-display text-tx-0">
          {t('folders.create_title')}
        </h3>
        <p className="mt-1 font-sans text-xs text-tx-2">
          {t('folders.create_description')}
        </p>
      </div>
      <div className="grid gap-4">
        <FormField label={t('folders.name_label')} required>
          <FormInput
            value={name}
            onChange={(event) => onNameChange(event.target.value)}
            placeholder={t('folders.name_placeholder')}
            autoFocus
            disabled={pending || disabled}
            disabledReason={!pending ? disabledReason : undefined}
          />
        </FormField>
        <FormField label={t('folders.parent_label')}>
          <FormSelect
            value={parent}
            onChange={onParentChange}
            options={parentOptions}
            disabled={disabled}
            disabledReason={disabledReason}
            className="w-full"
          />
        </FormField>
      </div>
      <div className="mt-6 flex justify-end gap-2">
        <ChromeButton type="button" onClick={onCancel}>
          {t('folders.cancel')}
        </ChromeButton>
        <ChromeButton
          type="submit"
          variant="primary"
          disabled={!name.trim() || pending || disabled}
          disabledReason={!pending ? disabledReason : undefined}
        >
          {pending ? t('folders.creating') : t('folders.create')}
        </ChromeButton>
      </div>
    </form>
  );
}

export function FolderDetails({
  folder,
  allLabel,
  childCount,
  dashboardCount,
  panelCount,
  editName,
  editParent,
  parentOptions,
  saving,
  editDisabled,
  editDisabledReason,
  createChildDisabled,
  createChildDisabledReason,
  deleteDisabled,
  deleteDisabledReason,
  path,
  onEditNameChange,
  onEditParentChange,
  onOpen,
  onCreateChild,
  onSave,
  onDelete,
}: {
  folder: FolderSummary | undefined;
  allLabel: string;
  childCount: number;
  dashboardCount: number;
  panelCount: number;
  editName: string;
  editParent: string;
  parentOptions: Array<{ value: string; label: string }>;
  saving: boolean;
  editDisabled: boolean;
  editDisabledReason: string | undefined;
  createChildDisabled: boolean;
  createChildDisabledReason: string;
  deleteDisabled: boolean;
  deleteDisabledReason: string | undefined;
  path: string;
  onEditNameChange: (value: string) => void;
  onEditParentChange: (value: string) => void;
  onOpen: () => void;
  onCreateChild: () => void;
  onSave: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation('dashboards');
  const isRoot = folder === undefined;
  const isDefault = folder?.id === DEFAULT_FOLDER;
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <p className="truncate font-sans text-xs text-tx-3">{path}</p>
          <h3 className="mt-1 truncate font-sans text-lg font-display text-tx-0">
            {folder?.name ?? allLabel}
          </h3>
        </div>
        <ChromeButton onClick={onOpen}>
          <FolderOpen className="h-3.5 w-3.5" />
          {t('folders.open_folder')}
        </ChromeButton>
      </div>

      <div className="mt-5 grid grid-cols-3 divide-x divide-bd-0 rounded-md border border-bd-0 bg-bg-1">
        <FolderMetric
          label={t('folders.dashboard_metric')}
          value={dashboardCount}
        />
        <FolderMetric
          label={t('folders.panel_metric')}
          value={panelCount}
        />
        <FolderMetric
          label={t('folders.child_metric')}
          value={childCount}
        />
      </div>

      {isRoot || isDefault ? (
        <div className="mt-5 rounded-md border border-bd-0 bg-bg-1 p-4">
          <p className="font-sans text-sm font-strong text-tx-0">
            {isRoot
              ? t('folders.overview_title')
              : t('folders.default_title')}
          </p>
          <p className="mt-1 max-w-xl font-sans text-xs leading-5 text-tx-2">
            {isRoot
              ? t('folders.overview_description')
              : t('folders.default_description')}
          </p>
        </div>
      ) : (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            onSave();
          }}
          className="mt-5 grid gap-4"
        >
          <FormField label={t('folders.name_label')} required>
            <FormInput
              value={editName}
              onChange={(event) => onEditNameChange(event.target.value)}
              disabled={saving || editDisabled}
              disabledReason={!saving ? editDisabledReason : undefined}
            />
          </FormField>
          <FormField label={t('folders.parent_label')}>
            <FormSelect
              value={editParent}
              onChange={onEditParentChange}
              options={parentOptions}
              disabled={editDisabled}
              disabledReason={editDisabledReason}
              className="w-full"
            />
          </FormField>
          <div className="flex justify-end">
            <ChromeButton
              type="submit"
              variant="primary"
              disabled={!editName.trim() || saving || editDisabled}
              disabledReason={!saving ? editDisabledReason : undefined}
            >
              {saving ? t('folders.saving') : t('folders.save_changes')}
            </ChromeButton>
          </div>
        </form>
      )}

      <div className="mt-auto flex items-center justify-between border-t border-bd-0 pt-4">
        <ChromeButton
          disabled={isDefault || createChildDisabled || editDisabled}
          disabledReason={
            isDefault
              ? t('folders.default_read_only')
              : createChildDisabled
                ? createChildDisabledReason
                : editDisabledReason
          }
          onClick={onCreateChild}
        >
          <Plus className="h-3.5 w-3.5" />
          {t(isRoot ? 'folders.new_folder' : 'folders.new_subfolder')}
        </ChromeButton>
        {!isRoot && !isDefault && (
          <ChromeButton
            disabled={deleteDisabled}
            disabledReason={deleteDisabledReason}
            onClick={onDelete}
          >
            <Trash2 className="h-3.5 w-3.5 text-red" />
            {t('folders.delete')}
          </ChromeButton>
        )}
      </div>
    </div>
  );
}

function FolderMetric({ label, value }: { label: string; value: number }) {
  return (
    <div className="px-4 py-3">
      <p className="font-mono text-lg font-display text-tx-0">{value}</p>
      <p className="font-sans text-xs text-tx-3">{label}</p>
    </div>
  );
}
