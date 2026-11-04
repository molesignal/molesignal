import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as automationsApi from '@/api/agent/automations';
import type { Automation, AutomationToken } from '@/api/agent/automations';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { CopyField } from '@/shell/CopyField';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';

import { AUTOMATIONS_KEY } from './AutomationEditor';
import { automationTriggerPath, curlExample } from './model';

/**
 * The token an outside system starts an automation with. A new token is shown
 * once, with the request that uses it; making another takes the old one away.
 */
export function TokenDialog({
  automation,
  onClose,
}: {
  automation: Automation | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const [issued, setIssued] = React.useState<AutomationToken | null>(null);

  React.useEffect(() => setIssued(null), [automation?.id]);

  const rotate = useMutation({
    mutationFn: () => automationsApi.rotateAutomationToken(automation!.id),
    onSuccess: (token) => {
      setIssued(token);
      void queryClient.invalidateQueries({ queryKey: AUTOMATIONS_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const revoke = useMutation({
    mutationFn: () => automationsApi.revokeAutomationToken(automation!.id),
    onSuccess: () => {
      toast.success(t('automations.token.revoked'));
      void queryClient.invalidateQueries({ queryKey: AUTOMATIONS_KEY });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (!automation) return null;

  const path = issued?.triggerPath ?? automationTriggerPath(automation.id);
  const busy = rotate.isPending || revoke.isPending;

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="w-[min(600px,calc(100vw-24px))] max-w-none">
        <DialogHeader>
          <DialogTitle>{t('automations.token.title', { name: automation.name })}</DialogTitle>
          <DialogDescription>
            {issued ? t('automations.token.shown_once') : t('automations.token.description')}
          </DialogDescription>
        </DialogHeader>
        <div data-automation-token className="flex min-w-0 flex-col gap-3 font-sans text-xs">
          {issued ? (
            <>
              <CopyField value={issued.token} copyLabel={t('automations.token.copy')} />
              <div className="flex flex-col gap-1.5">
                <span className="text-tx-2">{t('automations.token.example')}</span>
                <pre className="m-0 whitespace-pre-wrap rounded-md bg-bg-2 px-3 py-2.5 text-xs leading-relaxed text-tx-1 [overflow-wrap:anywhere]">
                  {curlExample(window.location.origin, path, issued.token)}
                </pre>
              </div>
            </>
          ) : (
            <p className="m-0 leading-relaxed text-tx-1">
              {automation.apiTokenPrefix
                ? t('automations.token.current', { prefix: automation.apiTokenPrefix })
                : t('automations.token.none')}
            </p>
          )}
        </div>
        <DialogFooter>
          {!issued && automation.apiTokenPrefix && (
            <ChromeButton className="mr-auto" disabled={busy} onClick={() => revoke.mutate()}>
              {t('automations.token.revoke')}
            </ChromeButton>
          )}
          {issued ? (
            <ChromeButton variant="primary" onClick={onClose}>
              {t('common.close')}
            </ChromeButton>
          ) : (
            <ChromeButton variant="primary" disabled={busy} onClick={() => rotate.mutate()}>
              {automation.apiTokenPrefix ? t('automations.token.rotate') : t('automations.token.create')}
            </ChromeButton>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
