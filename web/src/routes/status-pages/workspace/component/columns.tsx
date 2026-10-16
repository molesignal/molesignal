import type { TFunction } from 'i18next';
import { Archive, ArchiveRestore, ArrowDown, ArrowUp, Pencil, Trash2 } from 'lucide-react';
import type * as React from 'react';

import type { DataTableColumn } from '@/admin';
import type { StatusPageComponent } from '@/api/statusPages';
import { IconButton, Pill } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';

import type { ComponentListRow } from './rows';
import { COMPONENT_TONE, componentStatusLabel, isComponentGroup } from '../../model';

export interface ComponentColumnActions {
  canMutate: boolean;
  disabledReason: React.ReactNode;
  busy: { ordering: boolean; archiving: boolean };
  /** The components a row moves up and down among. */
  siblingsOf: (component: StatusPageComponent) => StatusPageComponent[];
  onMove: (component: StatusPageComponent, direction: -1 | 1) => void;
  onEdit: (component: StatusPageComponent) => void;
  onArchive: (component: StatusPageComponent) => void;
  onRestore: (component: StatusPageComponent) => void;
  onDelete: (component: StatusPageComponent) => void;
}

/** The columns of the components list: name, status, the business service it stands for, and what can be done. */
export function componentColumns({
  t,
  actions,
  serviceNames,
}: {
  t: TFunction;
  actions: ComponentColumnActions;
  /** Names of business services, when the list shows which one each component stands for. */
  serviceNames: Map<string, string> | null;
}): DataTableColumn<ComponentListRow>[] {
  return [
    {
      key: 'name',
      header: t('columns.component'),
      cell: ({ component, depth, members }) => (
        <div className={cn('min-w-0', depth === 1 && 'border-l border-bd-1 pl-4')}>
          <div className="flex min-w-0 items-center gap-2">
            <span className={cn('truncate text-tx-0', isComponentGroup(component) && 'font-strong')}>
              {component.name}
            </span>
            {isComponentGroup(component) && (
              <span className="shrink-0 text-xs text-tx-3">
                {t('values.group_components', { count: members.length })}
              </span>
            )}
            {/* Being on the public page is the rule; only the exception is marked. */}
            {component.visibility === 'hidden' && <Pill tone="dim">{t('component_visibility.hidden')}</Pill>}
          </div>
          {component.description && (
            <div className="mt-0.5 truncate text-xs text-tx-2">{component.description}</div>
          )}
        </div>
      ),
    },
    {
      key: 'status',
      header: t('columns.status'),
      width: 180,
      cell: ({ component, members, status }) =>
        isComponentGroup(component) && members.length === 0 ? (
          <span className="text-xs text-tx-3">{t('values.empty_group')}</span>
        ) : (
          <Pill tone={COMPONENT_TONE[status]}>{componentStatusLabel(t, status)}</Pill>
        ),
    },
    ...(serviceNames
      ? [{
          key: 'business_service',
          header: t('columns.business_service'),
          width: 220,
          cell: ({ component }: ComponentListRow) => {
            const link = component.business_service;
            if (!link) return <span className="text-tx-3">{t('values.none')}</span>;
            return (
              <div className="flex min-w-0 items-center gap-2">
                <span className="truncate text-tx-1">
                  {serviceNames.get(link.business_service_id) ?? t('forms.component.unknown_business_service')}
                </span>
                <Pill tone="dim">{t(`service_link_mode.${link.mode}_short`)}</Pill>
              </div>
            );
          },
        }]
      : []),
    {
      key: 'actions',
      header: t('columns.actions'),
      width: 220,
      cell: ({ component, members }) => (
        <div className="flex items-center gap-0.5" onClick={(event) => event.stopPropagation()}>
          {component.lifecycle === 'active'
            ? <ActiveActions component={component} members={members} actions={actions} t={t} />
            : <ArchivedActions component={component} actions={actions} t={t} />}
        </div>
      ),
    },
  ];
}

function ActiveActions({
  component,
  members,
  actions,
  t,
}: {
  component: StatusPageComponent;
  members: StatusPageComponent[];
  actions: ComponentColumnActions;
  t: TFunction;
}) {
  const siblings = actions.siblingsOf(component);
  // A group keeps its components; it is archived once they are out of it or archived.
  const holdsComponents = isComponentGroup(component) && members.some((member) => member.lifecycle === 'active');

  return (
    <>
      <IconButton
        aria-label={t('actions.move_up')}
        title={t('actions.move_up')}
        disabled={!actions.canMutate || siblings[0]?.id === component.id || actions.busy.ordering}
        disabledReason={actions.disabledReason}
        onClick={() => actions.onMove(component, -1)}
      >
        <ArrowUp className="h-3.5 w-3.5" />
      </IconButton>
      <IconButton
        aria-label={t('actions.move_down')}
        title={t('actions.move_down')}
        disabled={!actions.canMutate || siblings.at(-1)?.id === component.id || actions.busy.ordering}
        disabledReason={actions.disabledReason}
        onClick={() => actions.onMove(component, 1)}
      >
        <ArrowDown className="h-3.5 w-3.5" />
      </IconButton>
      <IconButton
        aria-label={t('actions.edit_component', { name: component.name })}
        title={t('actions.edit_component', { name: component.name })}
        disabled={!actions.canMutate}
        disabledReason={actions.disabledReason}
        onClick={() => actions.onEdit(component)}
      >
        <Pencil className="h-3.5 w-3.5" />
      </IconButton>
      <IconButton
        aria-label={t('actions.archive')}
        title={holdsComponents ? t('forms.component.group_not_empty') : t('actions.archive')}
        disabled={!actions.canMutate || holdsComponents || actions.busy.archiving}
        disabledReason={holdsComponents ? t('forms.component.group_not_empty') : actions.disabledReason}
        onClick={() => actions.onArchive(component)}
      >
        <Archive className="h-3.5 w-3.5" />
      </IconButton>
    </>
  );
}

function ArchivedActions({
  component,
  actions,
  t,
}: {
  component: StatusPageComponent;
  actions: ComponentColumnActions;
  t: TFunction;
}) {
  return (
    <>
      <IconButton
        aria-label={t('actions.restore')}
        title={t('actions.restore')}
        disabled={!actions.canMutate || actions.busy.archiving}
        disabledReason={actions.disabledReason}
        onClick={() => actions.onRestore(component)}
      >
        <ArchiveRestore className="h-3.5 w-3.5" />
      </IconButton>
      <IconButton
        aria-label={t('actions.delete')}
        title={t('actions.delete')}
        disabled={!actions.canMutate}
        disabledReason={actions.disabledReason}
        className="enabled:hover:text-red-soft"
        onClick={() => actions.onDelete(component)}
      >
        <Trash2 className="h-3.5 w-3.5" />
      </IconButton>
    </>
  );
}
