import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as syntheticsApi from '@/api/synthetics';
import type {
  ProbeAgentToken,
  ProbeAgentTokenInstructions,
  ProbeLocation,
} from '@/api/synthetics';
import { writeClipboardText } from '@/lib/clipboard';
import { toApiError } from '@/lib/http';
import { CopyIconButton } from '@/shell/CopyIconButton';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

const EXPIRY_OPTIONS = [
  { value: '30', key: 'agent_tokens.expiry_30' },
  { value: '90', key: 'agent_tokens.expiry_90' },
  { value: '180', key: 'agent_tokens.expiry_180' },
  { value: '365', key: 'agent_tokens.expiry_365' },
  { value: '0', key: 'agent_tokens.expiry_never' },
] as const;

export function AgentTokenFormDrawer({
  open,
  onOpenChange,
  locations,
  rotateToken,
  onIssued,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  locations: ProbeLocation[];
  rotateToken?: ProbeAgentToken | undefined;
  onIssued: (instructions: ProbeAgentTokenInstructions) => void;
}) {
  const { t } = useTranslation('synthetics');
  const [name, setName] = React.useState('');
  const [locationId, setLocationId] = React.useState('');
  const [expiry, setExpiry] = React.useState('90');
  React.useEffect(() => {
    if (!open) return;
    setName('');
    setLocationId(locations[0]?.id ?? '');
    setExpiry('90');
  }, [locations, open]);
  const issue = useMutation({
    mutationFn: () => rotateToken
      ? syntheticsApi.rotateAgentToken(rotateToken.id, Number(expiry))
      : syntheticsApi.createAgentToken({
          name: name.trim(),
          location_id: locationId,
          expires_in_days: Number(expiry),
        }),
    onSuccess: (instructions) => {
      onOpenChange(false);
      onIssued(instructions);
      toast.success(t(rotateToken ? 'agent_tokens.rotated' : 'agent_tokens.created'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const invalid = rotateToken ? false : !name.trim() || !locationId;
  const formId = rotateToken ? 'rotate-agent-token' : 'create-agent-token';
  return (
    <FormDrawer
      open={open}
      onOpenChange={onOpenChange}
      title={t(rotateToken ? 'agent_tokens.rotate_title' : 'agent_tokens.create_title')}
      subtitle={t(
        rotateToken
          ? 'agent_tokens.rotate_subtitle'
          : 'agent_tokens.create_subtitle',
      )}
      footer={
        <FormSubmitFooter
          busy={issue.isPending}
          invalid={invalid}
          onCancel={() => onOpenChange(false)}
          formId={formId}
          submitLabel={t(rotateToken ? 'actions.rotate' : 'agent_tokens.create_action')}
        />
      }
    >
      <form
        id={formId}
        onSubmit={(event) => {
          event.preventDefault();
          if (!invalid) issue.mutate();
        }}
      >
        <FormSection description={t('agent_tokens.form_hint')}>
          {rotateToken ? (
            <div className="rounded-md bg-bg-2 px-3 py-2 text-sm text-tx-1">
              {rotateToken.name} · <span className="font-code">{rotateToken.token_prefix}…</span>
            </div>
          ) : (
            <>
              <FormField label={t('agent_tokens.name')} required>
                <FormInput
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                  placeholder={t('agent_tokens.name_placeholder')}
                />
              </FormField>
              <FormField label={t('agents.target_location')} required>
                <FormSelect
                  value={locationId}
                  onChange={setLocationId}
                  options={locations.map((location) => ({
                    value: location.id,
                    label: `${location.name} · ${location.code}`,
                  }))}
                />
              </FormField>
            </>
          )}
          <FormField label={t('agent_tokens.expiry')}>
            <FormSelect
              value={expiry}
              onChange={setExpiry}
              options={EXPIRY_OPTIONS.map((option) => ({
                value: option.value,
                label: t(option.key),
              }))}
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

export function AgentTokenInstructionsDrawer({
  instructions,
  onClose,
}: {
  instructions?: ProbeAgentTokenInstructions | undefined;
  onClose: () => void;
}) {
  const { t } = useTranslation('synthetics');
  return (
    <FormDrawer
      open={Boolean(instructions)}
      onOpenChange={(open) => !open && onClose()}
      title={t('agent_tokens.reveal_title')}
      subtitle={t('agent_tokens.reveal_hint')}
    >
      <FormSection title={t('agent_tokens.token')}>
        <CopyValue
          value={instructions?.agent_token ?? ''}
          label={t('agent_tokens.copy_token')}
        />
      </FormSection>
      <FormSection title={t('agent_tokens.command')}>
        <CopyValue
          value={instructions?.command ?? ''}
          label={t('actions.copy_command')}
          multiline
        />
      </FormSection>
    </FormDrawer>
  );
}

function CopyValue({
  value,
  label,
  multiline = false,
}: {
  value: string;
  label: string;
  multiline?: boolean;
}) {
  const { t } = useTranslation('synthetics');
  const [copied, setCopied] = React.useState(false);
  React.useEffect(() => setCopied(false), [value]);
  return (
    <div className="relative border-t border-bd-0 bg-bg-0 p-4 pr-12">
      <code
        className={
          multiline
            ? 'block whitespace-pre-wrap break-all font-code text-xs leading-relaxed text-tx-1'
            : 'block break-all font-code text-sm text-tx-1'
        }
      >
        {value}
      </code>
      <CopyIconButton
        label={label}
        copied={copied}
        copiedLabel={t('agent_tokens.copied')}
        className="absolute right-2 top-2"
        onClick={() => {
          void writeClipboardText(value).then(() => setCopied(true));
        }}
      />
    </div>
  );
}
