import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as nodeAgentsApi from '@/api/nodeAgents';
import type { IssuedEnrollmentToken } from '@/api/nodeAgents';
import { writeClipboardText } from '@/lib/clipboard';
import { toApiError } from '@/lib/http';
import { CopyableValue } from '@/routes/settings/_atoms';
import { CopyIconButton } from '@/shell/CopyIconButton';
import {
  FormDrawer,
  FormField,
  FormGroup,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { durationLabel } from '../Frame';
import { parseLabels } from '../model';

const TTL_MINUTES = [15, 60, 1440, 10080] as const;
const MAX_USES = [1, 5, 20, 100, 1000] as const;

/** A token agents enroll with: how long it lasts, how many nodes it enrolls, and their labels. */
export function CreateTokenDrawer({
  open,
  onClose,
  onIssued,
}: {
  open: boolean;
  onClose: () => void;
  onIssued: (token: IssuedEnrollmentToken) => void;
}) {
  const { t } = useTranslation('infrastructure');
  const [description, setDescription] = React.useState('');
  const [labelText, setLabelText] = React.useState('');
  const [ttl, setTtl] = React.useState('60');
  const [maxUses, setMaxUses] = React.useState('1');

  React.useEffect(() => {
    if (!open) return;
    setDescription('');
    setLabelText('');
    setTtl('60');
    setMaxUses('1');
  }, [open]);

  const labels = parseLabels(labelText);

  const create = useMutation({
    mutationFn: () =>
      nodeAgentsApi.createEnrollmentToken({
        description: description.trim(),
        labels: 'labels' in labels ? labels.labels : {},
        maxUses: Number(maxUses),
        ttlMinutes: Number(ttl),
      }),
    onSuccess: (token) => {
      toast.success(t('node_agents.enrollment.issued'));
      onIssued(token);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('node_agents.enrollment.create_title')}
      subtitle={t('node_agents.enrollment.create_subtitle')}
      width={560}
      dirty={Boolean(description || labelText)}
      footer={
        <FormSubmitFooter
          busy={create.isPending}
          invalid={'invalidLine' in labels}
          onCancel={onClose}
          formId="create-enrollment-token"
          submitLabel={t('node_agents.enrollment.create')}
        />
      }
    >
      <form
        id="create-enrollment-token"
        onSubmit={(event) => {
          event.preventDefault();
          if ('labels' in labels) create.mutate();
        }}
      >
        <FormSection>
          <FormField label={t('node_agents.enrollment.description')}>
            <FormInput
              value={description}
              maxLength={255}
              onChange={(event) => setDescription(event.target.value)}
              placeholder={t('node_agents.enrollment.description_placeholder')}
            />
          </FormField>
          <FormField label={t('node_agents.enrollment.ttl')}>
            <FormSelect
              value={ttl}
              onChange={setTtl}
              options={TTL_MINUTES.map((value) => ({
                value: String(value),
                label: durationLabel(t, value),
              }))}
            />
          </FormField>
          <FormField label={t('node_agents.enrollment.max_uses')} hint={t('node_agents.enrollment.max_uses_hint')}>
            <FormSelect
              value={maxUses}
              onChange={setMaxUses}
              options={MAX_USES.map((value) => ({ value: String(value), label: String(value) }))}
            />
          </FormField>
          <FormField label={t('node_agents.enrollment.labels')} hint={t('node_agents.enrollment.labels_hint')}>
            <FormTextarea
              value={labelText}
              rows={3}
              onChange={(event) => setLabelText(event.target.value)}
              placeholder="rack=r1"
              className="font-code"
            />
          </FormField>
          {'invalidLine' in labels && (
            <span className="text-xs text-red">
              {t('node_agents.enrollment.labels_invalid', { line: labels.invalidLine })}
            </span>
          )}
        </FormSection>
      </form>
    </FormDrawer>
  );
}

/** The token just issued: the one time it is shown, with where agents use it. */
export function IssuedTokenDrawer({
  token,
  onClose,
}: {
  token: IssuedEnrollmentToken | undefined;
  onClose: () => void;
}) {
  const { t } = useTranslation('infrastructure');
  const [copied, setCopied] = React.useState(false);
  React.useEffect(() => setCopied(false), [token]);
  const copyLabel = t('node_agents.enrollment.copy');
  const copiedLabel = t('node_agents.enrollment.copied');

  return (
    <FormDrawer
      open={Boolean(token)}
      onOpenChange={(open) => !open && onClose()}
      title={t('node_agents.enrollment.issued_title')}
      subtitle={t('node_agents.enrollment.issued_subtitle')}
      width={600}
    >
      {token && (
        <FormSection>
          <FormGroup label={t('node_agents.enrollment.token')}>
            <CopyableValue value={token.plaintext} copyLabel={copyLabel} copiedLabel={copiedLabel} />
          </FormGroup>
          <FormGroup label={t('node_agents.enrollment.endpoint')} hint={t('node_agents.enrollment.endpoint_hint')}>
            <CopyableValue value={token.endpoint} copyLabel={copyLabel} copiedLabel={copiedLabel} />
          </FormGroup>
          <FormGroup label={t('node_agents.enrollment.ca')} hint={t('node_agents.enrollment.ca_hint')}>
            <div className="relative rounded-md bg-bg-2 p-3 pr-12">
              <pre className="max-h-48 overflow-auto whitespace-pre font-code text-xs leading-relaxed text-tx-1">
                {token.caCertificatePem}
              </pre>
              <CopyIconButton
                label={copyLabel}
                copied={copied}
                wrapperClassName="absolute right-2 top-2"
                onClick={() => void writeClipboardText(token.caCertificatePem).then(() => setCopied(true))}
              />
            </div>
          </FormGroup>
        </FormSection>
      )}
    </FormDrawer>
  );
}
