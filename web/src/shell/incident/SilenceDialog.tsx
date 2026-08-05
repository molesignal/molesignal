import { useMutation, useQueryClient } from '@tanstack/react-query';
import { BellOff } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as mutesApi from '@/api/mutes';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormSelect, FormTextarea } from '@/shell/FormDrawer';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';

const DURATION_OPTIONS = [
  { value: '3600', labelKey: 'silence_incident.durations.one_hour' },
  { value: '14400', labelKey: 'silence_incident.durations.four_hours' },
  { value: '86400', labelKey: 'silence_incident.durations.one_day' },
  { value: '604800', labelKey: 'silence_incident.durations.one_week' },
] as const;

interface IncidentSilenceDialogProps {
  incidentId: string | null;
  incidentName?: string | undefined;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * Creates an id-scoped silence for one active incident. The backend injects a
 * reserved incident-id matcher while dispatching, so other incidents with the
 * same service/labels continue notifying.
 */
export function IncidentSilenceDialog({
  incidentId,
  incidentName,
  open,
  onOpenChange,
}: IncidentSilenceDialogProps) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const silenceAccess = useActionAccess({ permission: 'alerts.silence' });
  const [durationSecs, setDurationSecs] = React.useState('3600');
  const [comment, setComment] = React.useState('');

  React.useEffect(() => {
    if (!open) return;
    setDurationSecs('3600');
    setComment('');
  }, [open, incidentId]);

  const silence = useMutation({
    mutationFn: () =>
      mutesApi.silenceIncident(incidentId!, {
        duration_secs: Number(durationSecs),
        comment: comment.trim(),
      }),
    onSuccess: async () => {
      toast.success(
        t('silence_incident.success', {
          name: incidentName ?? t('silence_incident.default_name'),
        }),
      );
      await queryClient.invalidateQueries({ queryKey: ['alert-mutes'] });
      onOpenChange(false);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[min(420px,calc(100vw-24px))]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <BellOff className="h-4 w-4 text-yellow-soft" />
            {t('silence_incident.title')}
          </DialogTitle>
          <DialogDescription>
            {t('silence_incident.description', {
              name: incidentName ?? t('silence_incident.default_name'),
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4 py-1">
          <FormField label={t('silence_incident.duration')}>
            <FormSelect
              value={durationSecs}
              onChange={setDurationSecs}
              disabled={silenceAccess.disabled}
              disabledReason={silenceAccess.reason}
              options={DURATION_OPTIONS.map((option) => ({
                value: option.value,
                label: t(option.labelKey),
              }))}
            />
          </FormField>
          <FormField label={t('silence_incident.reason')}>
            <FormTextarea
              value={comment}
              onChange={(event) => setComment(event.target.value)}
              disabled={silenceAccess.disabled}
              disabledReason={silenceAccess.reason}
              rows={3}
              placeholder={t('silence_incident.reason_placeholder')}
            />
          </FormField>
          <p className="font-sans text-xs leading-relaxed text-tx-3">
            {t('silence_incident.scope_hint')}
          </p>
        </div>

        <DialogFooter>
          <ChromeButton onClick={() => onOpenChange(false)}>
            {t('silence_incident.cancel')}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={!incidentId || silence.isPending || silenceAccess.disabled}
            disabledReason={!silence.isPending ? silenceAccess.reason : undefined}
            onClick={() => {
              if (incidentId && silenceAccess.allowed) silence.mutate();
            }}
          >
            <BellOff className="h-3.5 w-3.5" />
            {silence.isPending
              ? t('silence_incident.silencing')
              : t('silence_incident.confirm')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
