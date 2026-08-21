import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as syntheticsApi from '@/api/synthetics';
import type { ProbeRegisterInstructions, ProbeLocation } from '@/api/synthetics';
import { writeClipboardText } from '@/lib/clipboard';
import { toApiError } from '@/lib/http';
import { CopyIconButton } from '@/shell/CopyIconButton';
import {
  FormDrawer,
  FormField,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

export function AgentRegisterDrawer({
  open,
  onOpenChange,
  locations,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  locations: ProbeLocation[];
  onCreated: (instructions: ProbeRegisterInstructions) => void;
}) {
  const { t } = useTranslation('synthetics');
  const [locationId, setLocationId] = React.useState('');
  const [ttlMinutes, setTtlMinutes] = React.useState('15');
  const defaultLocationId = locations[0]?.id ?? '';
  React.useEffect(() => {
    if (!open) return;
    setLocationId(defaultLocationId);
    setTtlMinutes('15');
  }, [defaultLocationId, open]);
  const create = useMutation({
    mutationFn: () => syntheticsApi.createRegisterToken(locationId, Number(ttlMinutes)),
    onSuccess: (instructions) => {
      onOpenChange(false);
      onCreated(instructions);
      toast.success(t('agents.register_created'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <FormDrawer
      open={open}
      onOpenChange={onOpenChange}
      title={t('agents.register_title')}
      subtitle={t('agents.register_subtitle')}
      footer={
        <FormSubmitFooter
          busy={create.isPending}
          invalid={!locationId}
          onCancel={() => onOpenChange(false)}
          formId="register-synthetic-agent"
          submitLabel={t('actions.register_agent')}
        />
      }
    >
      <form
        id="register-synthetic-agent"
        onSubmit={(event) => {
          event.preventDefault();
          if (locationId) create.mutate();
        }}
      >
        <FormSection description={t('agents.register_description')}>
          <FormField label={t('agents.target_location')} required>
            <FormSelect
              value={locationId}
              onChange={setLocationId}
              options={locations.map((location) => ({
                value: location.id,
                label: `${location.name} · ${location.code}`,
              }))}
              placeholder={t('agents.no_eligible_locations')}
            />
          </FormField>
          <FormField label={t('agents.token_ttl')} hint={t('agents.token_ttl_hint')}>
            <FormSelect
              value={ttlMinutes}
              onChange={setTtlMinutes}
              options={[5, 15, 30, 60].map((minutes) => ({
                value: String(minutes),
                label: t('agents.ttl_minutes', { count: minutes }),
              }))}
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

export function RegisterInstructionsDrawer({
  instructions,
  onClose,
}: {
  instructions: ProbeRegisterInstructions | undefined;
  onClose: () => void;
}) {
  const { t } = useTranslation('synthetics');
  const [copied, setCopied] = React.useState(false);
  React.useEffect(() => setCopied(false), [instructions]);
  return (
    <FormDrawer
      open={Boolean(instructions)}
      onOpenChange={(open) => !open && onClose()}
      title={t('locations.register_title')}
      subtitle={t('locations.register_hint')}
    >
      <div className="relative rounded-md border border-bd-0 bg-bg-0 p-4 pr-12">
        <pre className="overflow-x-auto whitespace-pre-wrap break-all font-code text-xs leading-relaxed text-tx-1">
          {instructions?.command}
        </pre>
        <CopyIconButton
          label={t('actions.copy_command')}
          copied={copied}
          className="absolute right-2 top-2"
          onClick={() => {
            if (!instructions) return;
            void writeClipboardText(instructions.command)
              .then(() => setCopied(true));
          }}
        />
      </div>
    </FormDrawer>
  );
}
