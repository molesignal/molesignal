import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as businessServicesApi from '@/api/businessServices';
import type {
  BusinessService,
  BusinessServiceDependency,
  BusinessServiceInput,
} from '@/api/businessServices';
import {
  FieldArray,
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';

import { useDependencyTargets } from '../targets';
import { DependencyRow } from './DependencyRow';

interface ServiceEditorProps {
  open: boolean;
  /** The service being changed; null for a new one. */
  service: BusinessService | null;
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (input: BusinessServiceInput) => void;
}

/**
 * A business service as its owner writes it: what customers call it and what
 * it depends on. Its health is worked out from those dependencies, so it is
 * not written here.
 *
 * The form lives for one opening and is made from what it was opened for, so
 * a select is never handed its value after it is drawn.
 */
export function ServiceEditor({ open, ...opening }: ServiceEditorProps) {
  return open ? <ServiceForm {...opening} /> : null;
}

function ServiceForm({ service, busy, onOpenChange, onSubmit }: Omit<ServiceEditorProps, 'open'>) {
  const { t } = useTranslation('business-services');
  const [name, setName] = React.useState(() => service?.name ?? '');
  const [description, setDescription] = React.useState(() => service?.description ?? '');
  const [dependencies, setDependencies] = React.useState<BusinessServiceDependency[]>(
    () => service?.dependencies ?? [],
  );
  const targets = useDependencyTargets(true, service?.id);
  const formId = 'business-service-form';

  const complete = dependencies.filter((dependency) => dependency.target.trim());
  const invalid = !name.trim()
    || complete.length !== dependencies.length
    || dependencies.length > businessServicesApi.MAX_BUSINESS_SERVICE_DEPENDENCIES;

  return (
    <FormDrawer
      open
      onOpenChange={onOpenChange}
      title={service ? t('editor.edit_title') : t('editor.create_title')}
      subtitle={t('editor.subtitle')}
      footer={
        <FormSubmitFooter
          busy={busy}
          invalid={invalid}
          onCancel={() => onOpenChange(false)}
          submitLabel={service ? t('actions.save') : t('actions.create')}
          formId={formId}
        />
      }
    >
      <form
        id={formId}
        onSubmit={(event) => {
          event.preventDefault();
          if (invalid) return;
          onSubmit({
            name: name.trim(),
            description: description.trim(),
            dependencies: complete.map((dependency) => ({ ...dependency, target: dependency.target.trim() })),
          });
        }}
      >
        <FormSection>
          <FormField label={t('fields.name')} required hint={t('editor.name_hint')}>
            <FormInput
              value={name}
              maxLength={businessServicesApi.MAX_BUSINESS_SERVICE_NAME_CHARS}
              onChange={(event) => setName(event.currentTarget.value)}
            />
          </FormField>
          <FormField label={t('fields.description')}>
            <FormTextarea
              value={description}
              maxLength={businessServicesApi.MAX_BUSINESS_SERVICE_DESCRIPTION_CHARS}
              onChange={(event) => setDescription(event.currentTarget.value)}
            />
          </FormField>
        </FormSection>
        <FormSection title={t('editor.dependencies')} description={t('editor.dependencies_hint')}>
          <FieldArray<BusinessServiceDependency>
            items={dependencies}
            onChange={setDependencies}
            newItem={() => ({ kind: 'technical_service', target: '', impact: 'outage' })}
            addLabel={t('actions.add_dependency')}
            removeLabel={t('actions.remove_dependency')}
            emptyHint={t('editor.no_dependencies')}
            renderItem={(dependency, _, setItem) => (
              <DependencyRow dependency={dependency} onChange={setItem} options={targets.options} />
            )}
          />
        </FormSection>
      </form>
    </FormDrawer>
  );
}
