import { Bot } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { incidentSubject } from '@/api/agent/references';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';
import type { Incident } from '@/types/alerting';

/**
 * Investigate an incident in a new Mole Agent chat: for the team the incident's
 * escalation policy names (which can still be changed before sending), with
 * the incident attached and a first question to read and send. The chat
 * records the investigation; its answer can be written back as the RCA.
 */
export function InvestigateWithAgent({
  incident,
  size = 'md',
}: {
  incident: Pick<Incident, 'id' | 'summary'>;
  size?: 'sm' | 'md';
}) {
  const { t } = useTranslation('agent');
  const access = useActionAccess({ permission: 'agent.use' });
  const [busy, setBusy] = React.useState(false);
  if (!access.allowed) return null;

  const start = async () => {
    setBusy(true);
    try {
      const subject = await incidentSubject(incident.id);
      useMoleAgentStore.getState().start({
        prompt: t('investigate.prefill'),
        teamId: subject.team?.id ?? null,
        subject: { kind: 'incident', id: incident.id },
        references: [{ kind: 'incident', id: incident.id, label: subject.incident.summary }],
      });
    } catch (error) {
      toast.error(toApiError(error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <ChromeButton
      size={size}
      onClick={() => void start()}
      disabled={busy}
      data-testid="incident-investigate"
    >
      <Bot className={size === 'sm' ? 'h-3 w-3' : 'h-3.5 w-3.5'} />
      {t('investigate.start')}
    </ChromeButton>
  );
}
