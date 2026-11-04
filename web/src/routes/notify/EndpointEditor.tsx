import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as notifyApi from '@/api/notify';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { PHONE_CONNECTOR_TYPES } from './connector/fields';

export function EndpointEditor({
  open,
  userId,
  endpoint,
  connectors,
  canVerify,
  onClose,
}: {
  open: boolean;
  userId: string;
  endpoint: notifyApi.UserNotifyEndpoint | null;
  connectors: notifyApi.NotifyConnector[];
  canVerify: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation('notify');
  const qc = useQueryClient();
  const options = React.useMemo(
    () =>
      connectors.filter(
        (connector) => connector.enabled && connector.capabilities.directUser,
      ),
    [connectors],
  );
  const [connectorId, setConnectorId] = React.useState('');
  const [identity, setIdentity] = React.useState('');
  const [displayName, setDisplayName] = React.useState('');
  const [enabled, setEnabled] = React.useState(true);
  const [verified, setVerified] = React.useState(false);
  // A phone connector calls or texts the identity, which is then a phone number.
  const phone = PHONE_CONNECTOR_TYPES.has(
    options.find((connector) => connector.id === connectorId)?.connectorType ?? '',
  );

  React.useEffect(() => {
    if (!open) return;
    setConnectorId(endpoint?.connectorId ?? options[0]?.id ?? '');
    setIdentity(endpoint?.externalIdentity ?? '');
    setDisplayName(endpoint?.displayName ?? '');
    setEnabled(endpoint?.enabled ?? true);
    setVerified(endpoint?.verified ?? false);
  }, [endpoint, open, options]);

  const save = useMutation({
    mutationFn: () => {
      const input: notifyApi.UserNotifyEndpointInput = {
        connectorId: connectorId,
        externalIdentity: identity,
        displayName: displayName.trim() || null,
        metadata: endpoint?.metadata ?? {},
        enabled,
        ...(canVerify ? { verified } : {}),
      };
      return endpoint
        ? notifyApi.updateEndpoint(userId, endpoint.id, input)
        : notifyApi.createEndpoint(userId, input);
    },
    onSuccess: () => {
      toast.success(t('common.saved'));
      void qc.invalidateQueries({ queryKey: ['notify', 'endpoints', userId] });
      void qc.invalidateQueries({ queryKey: ['notify', 'users'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={endpoint ? t('endpoints.edit_title') : t('endpoints.new_title')}
      footer={
        <>
          <ChromeButton className="h-11 md:h-9" onClick={onClose}>
            {t('common.cancel')}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            className="h-11 md:h-9"
            disabled={
              connectorId === '' || identity.trim() === '' || save.isPending
            }
            onClick={() => save.mutate()}
          >
            {save.isPending ? t('common.saving') : t('endpoints.save')}
          </ChromeButton>
        </>
      }
    >
      <FormSection>
        <FormField label={t('endpoints.connector')} required>
          <FormSelect
            value={connectorId}
            onChange={setConnectorId}
            options={options.map((connector) => ({
              value: connector.id,
              label: `${connector.name} · ${t(`connector_types.${connector.connectorType}`, {
                defaultValue: connector.connectorType,
              })}`,
            }))}
            className="h-11 md:h-9"
          />
        </FormField>
        <FormField
          label={t('endpoints.identity')}
          required
          {...(phone ? { hint: t('endpoints.phone_hint') } : {})}
        >
          <FormInput
            className="h-11 text-base md:h-9 md:text-sm"
            value={identity}
            onChange={(event) => setIdentity(event.target.value)}
          />
        </FormField>
        <FormField label={t('endpoints.display_name')}>
          <FormInput
            className="h-11 text-base md:h-9 md:text-sm"
            value={displayName}
            onChange={(event) => setDisplayName(event.target.value)}
          />
        </FormField>
        <label className="flex min-h-11 items-center justify-between gap-4 rounded-md border border-bd-0 bg-bg-2 px-3 text-sm text-tx-1">
          <span>{t('endpoints.enabled')}</span>
          <Switch checked={enabled} onCheckedChange={setEnabled} />
        </label>
        {canVerify && (
          <label className="flex min-h-11 items-center justify-between gap-4 rounded-md border border-bd-0 bg-bg-2 px-3 text-sm text-tx-1">
            <span>{t('endpoints.verified')}</span>
            <Switch checked={verified} onCheckedChange={setVerified} />
          </label>
        )}
      </FormSection>
    </FormDrawer>
  );
}
