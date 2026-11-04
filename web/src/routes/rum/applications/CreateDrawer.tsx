import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import { type RumApplication, type RumPlatform, RUM_PLATFORMS } from '@/api/rum';
import { toApiError } from '@/lib/http';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';

import { applicationIdFromName, isApplicationId, MAX_APPLICATION_ID } from './model';

/** The form for a new application: a name, and if it is wanted an identifier and the platform. */
export function CreateApplicationDrawer({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The application was created: the page opens it for its token. */
  onCreated: (application: RumApplication) => void;
}) {
  const { t } = useTranslation('rum');
  const client = useQueryClient();
  const [name, setName] = React.useState('');
  const [id, setId] = React.useState('');
  const [platform, setPlatform] = React.useState<RumPlatform>('browser');
  const formId = React.useId();

  const create = useMutation({
    mutationFn: () =>
      rumApi.createApplication({
        name: name.trim(),
        platform,
        ...(id.trim() ? { id: id.trim() } : {}),
      }),
    onSuccess: async (application) => {
      await client.invalidateQueries({ queryKey: ['rum', 'applications'] });
      onOpenChange(false);
      onCreated(application);
    },
  });
  const { reset } = create;

  // Another visit starts from a blank form.
  React.useEffect(() => {
    if (open) return;
    setName('');
    setId('');
    setPlatform('browser');
    reset();
  }, [open, reset]);

  const typedId = id.trim();
  const idInvalid = typedId !== '' && !isApplicationId(typedId);
  const invalid = name.trim() === '' || idInvalid;
  const preview = applicationIdFromName(name);

  return (
    <FormDrawer
      open={open}
      onOpenChange={onOpenChange}
      width={520}
      title={t('applications.create.title')}
      subtitle={t('applications.create.subtitle')}
      footer={
        <FormSubmitFooter
          formId={formId}
          busy={create.isPending}
          invalid={invalid}
          onCancel={() => onOpenChange(false)}
          submitLabel={t('applications.create.submit')}
        />
      }
    >
      <form
        id={formId}
        onSubmit={(event) => {
          event.preventDefault();
          if (!invalid && !create.isPending) create.mutate();
        }}
      >
        <FormSection>
          <FormField label={t('applications.create.name')} required>
            <FormInput
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t('applications.create.name_placeholder')}
              maxLength={128}
              autoComplete="off"
              autoFocus
            />
          </FormField>
          <FormField
            label={t('applications.create.id')}
            hint={idInvalid ? t('applications.create.id_invalid') : t('applications.create.id_hint', { id: preview })}
          >
            <FormInput
              value={id}
              onChange={(event) => setId(event.target.value)}
              placeholder={preview}
              maxLength={MAX_APPLICATION_ID}
              autoComplete="off"
              aria-invalid={idInvalid || undefined}
            />
          </FormField>
          <FormField label={t('applications.create.platform')}>
            <FormSelect
              value={platform}
              onChange={(value) => setPlatform(value as RumPlatform)}
              ariaLabel={t('applications.create.platform')}
              options={RUM_PLATFORMS.map((value) => ({ value, label: t(`applications.platforms.${value}`) }))}
            />
          </FormField>
        </FormSection>
        {create.isError && (
          <p role="alert" className="m-0 rounded-md bg-red-dim px-3 py-2 font-sans text-sm text-red-soft">
            {toApiError(create.error).message}
          </p>
        )}
      </form>
    </FormDrawer>
  );
}
