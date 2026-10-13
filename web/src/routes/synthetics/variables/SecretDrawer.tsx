import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as syntheticsApi from '@/api/synthetics';
import type { SyntheticSecret } from '@/api/synthetics';
import { toApiError } from '@/lib/http';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

const FORM_ID = 'synthetic-secret-form';

/**
 * A new secret, or a new value for one that exists. A secret's name is what
 * checks refer to, so rotating changes the value and nothing else.
 */
export function SecretDrawer({
  open,
  onOpenChange,
  secret,
  onSaved,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  secret?: SyntheticSecret | undefined;
  onSaved: () => void | Promise<void>;
}) {
  const { t } = useTranslation('synthetics');
  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [value, setValue] = React.useState('');

  React.useEffect(() => {
    if (!open) return;
    setName(secret?.name ?? '');
    setDescription(secret?.description ?? '');
    setValue('');
  }, [open, secret]);

  const save = useMutation({
    mutationFn: () =>
      secret
        ? syntheticsApi.rotateSecret(secret.id, value)
        : syntheticsApi.createSecret({
            name: name.trim(),
            description: description.trim(),
            value,
          }),
    onSuccess: async () => {
      toast.success(t(secret ? 'variables.rotated' : 'variables.created'));
      onOpenChange(false);
      await onSaved();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const invalid = (!secret && !name.trim()) || !value;

  return (
    <FormDrawer
      open={open}
      onOpenChange={onOpenChange}
      title={secret ? t('variables.rotate_title', { name: secret.name }) : t('variables.create_title')}
      subtitle={t('variables.secret_hint')}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          invalid={invalid}
          onCancel={() => onOpenChange(false)}
          formId={FORM_ID}
          submitLabel={secret ? t('actions.rotate') : t('actions.create_secret')}
        />
      }
    >
      <form
        id={FORM_ID}
        onSubmit={(event) => {
          event.preventDefault();
          if (!invalid) save.mutate();
        }}
      >
        <FormSection>
          {!secret && (
            <>
              <FormField label={t('variables.name')} required>
                <FormInput
                  value={name}
                  onChange={(event) =>
                    setName(event.target.value.toUpperCase().replace(/[^A-Z0-9_]/g, '_'))
                  }
                  placeholder="API_KEY"
                />
              </FormField>
              <FormField label={t('variables.description')}>
                <FormTextarea
                  value={description}
                  onChange={(event) => setDescription(event.target.value)}
                />
              </FormField>
            </>
          )}
          <FormField label={t('variables.value')} required>
            <FormInput
              type="password"
              autoComplete="new-password"
              value={value}
              onChange={(event) => setValue(event.target.value)}
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
