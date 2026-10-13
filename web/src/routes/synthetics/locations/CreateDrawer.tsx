import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as syntheticsApi from '@/api/synthetics';
import type { ProbeLocation } from '@/api/synthetics';
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

const CODE_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const FORM_ID = 'create-synthetic-location';

/** A private location: a pool of agents behind a firewall, and what they may reach. */
export function CreateLocationDrawer({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated: (location: ProbeLocation) => void;
}) {
  const { t } = useTranslation('synthetics');
  const [name, setName] = React.useState('');
  const [code, setCode] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [allowedDomains, setAllowedDomains] = React.useState('');
  const [allowPrivate, setAllowPrivate] = React.useState(true);
  const create = useMutation({
    mutationFn: () =>
      syntheticsApi.createLocation({
        name,
        code,
        description,
        egress_policy: {
          allowed_cidrs: [],
          denied_cidrs: [],
          allowed_domains: allowedDomains
            .split(',')
            .map((value) => value.trim())
            .filter(Boolean),
          denied_domains: [],
          allowed_ports: [],
          allow_private_networks: allowPrivate,
          allow_loopback: false,
        },
      }),
    onSuccess: onCreated,
    onError: (error) => toast.error(toApiError(error).message),
  });

  React.useEffect(() => {
    if (!open) return;
    setName('');
    setCode('');
    setDescription('');
    setAllowedDomains('');
    setAllowPrivate(true);
  }, [open]);

  return (
    <FormDrawer
      open={open}
      onOpenChange={onOpenChange}
      title={t('locations.create_title')}
      subtitle={t('locations.create_subtitle')}
      footer={
        <FormSubmitFooter
          busy={create.isPending}
          invalid={!name.trim() || !CODE_PATTERN.test(code)}
          onCancel={() => onOpenChange(false)}
          formId={FORM_ID}
          submitLabel={t('actions.create_location')}
        />
      }
    >
      <form
        id={FORM_ID}
        onSubmit={(event) => {
          event.preventDefault();
          create.mutate();
        }}
      >
        <FormSection>
          <FormField label={t('locations.name')} required>
            <FormInput value={name} onChange={(event) => setName(event.target.value)} />
          </FormField>
          <FormField label={t('locations.code')} required>
            <FormInput
              value={code}
              onChange={(event) => setCode(event.target.value.toLowerCase())}
              placeholder="sg-private-1"
            />
          </FormField>
          <FormField label={t('locations.description')}>
            <FormTextarea
              value={description}
              onChange={(event) => setDescription(event.target.value)}
            />
          </FormField>
          <FormField
            label={t('locations.allowed_domains')}
            hint={t('locations.allowed_domains_hint')}
          >
            <FormInput
              value={allowedDomains}
              onChange={(event) => setAllowedDomains(event.target.value)}
            />
          </FormField>
          <label className="flex min-h-11 cursor-pointer items-center gap-3 rounded-md bg-[var(--control-surface)] px-3 font-sans text-sm text-tx-1">
            <input
              type="checkbox"
              checked={allowPrivate}
              onChange={(event) => setAllowPrivate(event.target.checked)}
              className="h-4 w-4 accent-indigo"
            />
            {t('locations.allow_private')}
          </label>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
