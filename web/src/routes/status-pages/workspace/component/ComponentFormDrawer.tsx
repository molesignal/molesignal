import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type {
  ComponentKind,
  ComponentServiceLink,
  ComponentStatus,
  StatusPageComponent,
  StatusPageComponentInput,
} from '@/api/statusPages';
import { useActionAccess } from '@/product/actionAccess';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRadio,
  FormRow,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';

import { COMPONENT_STATUSES, componentStatusLabel } from '../../model';
import { NotifySwitch } from '../NotifySwitch';
import { ServiceLinkFields } from './ServiceLinkFields';

export interface ComponentSubmission {
  input: StatusPageComponentInput;
  /** The business service link to set, null to remove it, undefined to leave it as it is. */
  link: ComponentServiceLink | null | undefined;
}

interface ComponentFormDrawerProps {
  open: boolean;
  component: StatusPageComponent | null;
  /** The groups of the page a component can go in. */
  groups: StatusPageComponent[];
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (submission: ComponentSubmission) => void;
}

/**
 * A component of the page, or a group of components, new or being changed.
 * The form lives for one opening and is made from what it was opened for:
 * its selects get their values when they are drawn, not after, and a refresh
 * of the page behind does not write over what is being typed.
 */
export function ComponentFormDrawer({ open, ...opening }: ComponentFormDrawerProps) {
  return open ? <ComponentForm {...opening} /> : null;
}

function ComponentForm({ component, groups, busy, onOpenChange, onSubmit }: Omit<ComponentFormDrawerProps, 'open'>) {
  const { t } = useTranslation('status-pages');
  const businessServiceAccess = useActionAccess({ permission: 'business_services.read' });
  // What it is stays as it was made.
  const [kind, setKind] = React.useState<ComponentKind>(() => component?.kind ?? 'component');
  const [name, setName] = React.useState(() => component?.name ?? '');
  const [description, setDescription] = React.useState(() => component?.description ?? '');
  const [status, setStatus] = React.useState<ComponentStatus>(() => component?.status ?? 'operational');
  const [visibility, setVisibility] = React.useState<'enabled' | 'hidden'>(
    () => component?.visibility ?? 'enabled',
  );
  const [lifecycle, setLifecycle] = React.useState<'active' | 'archived'>(
    () => component?.lifecycle ?? 'active',
  );
  const [groupId, setGroupId] = React.useState(() => component?.group_id ?? '');
  const [notify, setNotify] = React.useState(true);
  const [link, setLink] = React.useState<ComponentServiceLink | null>(
    () => component?.business_service ?? null,
  );
  const formId = 'status-page-component-form';
  const group = kind === 'group';
  const following = link?.mode === 'follow';
  const groupChoices = groups.filter((candidate) => candidate.id !== component?.id);

  return (
    <FormDrawer
      open
      onOpenChange={onOpenChange}
      title={t(component
        ? group ? 'forms.component.edit_group_title' : 'forms.component.edit_title'
        : 'forms.component.create_title')}
      subtitle={t(group ? 'forms.component.group_subtitle' : 'forms.component.subtitle')}
      footer={
        <FormSubmitFooter
          busy={busy}
          invalid={!name.trim()}
          onCancel={() => onOpenChange(false)}
          submitLabel={component ? t('actions.save') : t(group ? 'actions.add_group' : 'actions.add_component')}
          formId={formId}
        />
      }
    >
      <form
        id={formId}
        onSubmit={(event) => {
          event.preventDefault();
          if (!name.trim()) return;
          onSubmit({
            input: {
              name: name.trim(),
              description: description.trim(),
              status: group ? 'operational' : status,
              visibility: group ? 'enabled' : visibility,
              lifecycle: component ? lifecycle : 'active',
              ...(component ? { position: component.position } : {}),
              notify,
              kind,
              group_id: group ? null : groupId || null,
            },
            link: !group && businessServiceAccess.allowed
              && !linkUnchanged(component?.business_service ?? null, link)
              ? link
              : undefined,
          });
        }}
      >
        {!component && (
          <FormSection title={t('forms.component.kind')}>
            <FormRadio<ComponentKind>
              value={kind}
              onChange={setKind}
              options={(['component', 'group'] as const).map((option) => ({
                value: option,
                label: t(`component_kind.${option}`),
                hint: t(`component_kind.${option}_hint`),
              }))}
            />
          </FormSection>
        )}
        <FormSection>
          <FormField label={t(group ? 'fields.group_name' : 'fields.component_name')} required>
            <FormInput value={name} maxLength={120} onChange={(event) => setName(event.currentTarget.value)} />
          </FormField>
          <FormField label={t('fields.description')} hint={t('forms.component.description_hint')}>
            <FormTextarea
              value={description}
              maxLength={500}
              onChange={(event) => setDescription(event.currentTarget.value)}
            />
          </FormField>
          {!group && groupChoices.length > 0 && (
            <FormField label={t('fields.component_group')}>
              <FormSelect
                value={groupId}
                onChange={setGroupId}
                options={[
                  { value: '', label: t('forms.component.no_group') },
                  ...groupChoices.map((candidate) => ({ value: candidate.id, label: candidate.name })),
                ]}
              />
            </FormField>
          )}
          {!group && (
            <FormField
              label={t('fields.component_status')}
              {...(following ? { hint: t('forms.component.follow_status_hint') } : {})}
            >
              <FormSelect
                value={status}
                onChange={(value) => setStatus(value as ComponentStatus)}
                options={COMPONENT_STATUSES.map((value) => ({
                  value,
                  label: componentStatusLabel(t, value),
                }))}
              />
            </FormField>
          )}
          {!group && component && status !== component.status && (
            <NotifySwitch
              label={t('fields.notify_subscribers')}
              hint={t(notify ? 'forms.component.notify_hint' : 'forms.component.silent_hint')}
              checked={notify}
              onCheckedChange={setNotify}
            />
          )}
          {(!group || component) && (
            <FormRow className={!group && component ? 'grid-cols-1 sm:grid-cols-2' : 'grid-cols-1'}>
              {!group && (
                <FormField label={t('fields.component_visibility')}>
                  <FormSelect
                    value={visibility}
                    onChange={(value) => setVisibility(value as 'enabled' | 'hidden')}
                    options={[
                      { value: 'enabled', label: t('component_visibility.enabled') },
                      { value: 'hidden', label: t('component_visibility.hidden') },
                    ]}
                  />
                </FormField>
              )}
              {component && (
                <FormField label={t('fields.component_lifecycle')}>
                  <FormSelect
                    value={lifecycle}
                    onChange={(value) => setLifecycle(value as 'active' | 'archived')}
                    options={[
                      { value: 'active', label: t('lifecycle.active') },
                      { value: 'archived', label: t('lifecycle.archived') },
                    ]}
                  />
                </FormField>
              )}
            </FormRow>
          )}
        </FormSection>
        {!group && businessServiceAccess.allowed && lifecycle === 'active' && (
          <ServiceLinkFields link={link} onChange={setLink} />
        )}
      </form>
    </FormDrawer>
  );
}

function linkUnchanged(before: ComponentServiceLink | null, after: ComponentServiceLink | null): boolean {
  if (!before || !after) return before === after;
  return before.business_service_id === after.business_service_id
    && before.mode === after.mode
    && before.notify_subscribers === after.notify_subscribers;
}
