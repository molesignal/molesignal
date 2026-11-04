import { Bot } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { MessageReference } from '@/api/agent/chat';
import { incidentSubject } from '@/api/agent/references';
import { toApiError } from '@/lib/http';
import { useTimeFormatter } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { rumPrompt, rumReference } from '@/routes/rum/agent/prompt';
import { useApplicationRef } from '@/routes/rum/agent/useApplication';
import { incidentRange } from '@/routes/rum/alerts/links';
import { incidentFigure, sessionSamples } from '@/routes/rum/alerts/samples';
import { ChromeButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';
import type { AlertRule, Incident } from '@/types/alerting';

/** How many of the sessions an alert was told by are named in the question. */
const MOST_SESSIONS = 5;

/**
 * Investigate an incident in a new Mole Agent chat: for the team the incident's
 * escalation policy names (which can still be changed before sending), with
 * the incident attached and a first question to read and send. The chat
 * records the investigation; its answer can be written back as the RCA.
 *
 * An incident of a rule on a figure of a RUM application also attaches the
 * application, and the question names it, the time of the incident and the
 * sessions it was told by.
 */
export function InvestigateWithAgent({
  incident,
  rule,
  size = 'md',
  onStarted,
}: {
  incident: Pick<Incident, 'id' | 'summary'> &
    Partial<Pick<Incident, 'labels' | 'createdAt' | 'resolvedAt' | 'triggeringQuery'>>;
  rule?: Pick<AlertRule, 'query'> | undefined;
  size?: 'sm' | 'md';
  /** What to do once the chat is started: a drawer over the panel closes, so the question can be reached. */
  onStarted?: (() => void) | undefined;
}) {
  const { t } = useTranslation('agent');
  const { t: rumT } = useTranslation('rum');
  const time = useTimeFormatter();
  const access = useActionAccess({ permission: 'agent.use' });
  const [busy, setBusy] = React.useState(false);
  const figure = incidentFigure({ labels: incident.labels ?? {} }, rule);
  const application = useApplicationRef(figure?.application ?? '');
  if (!access.allowed) return null;

  const start = async () => {
    setBusy(true);
    try {
      const subject = await incidentSubject(incident.id);
      const references: MessageReference[] = [{ kind: 'incident', id: incident.id, label: subject.incident.summary }];
      let prompt = t('investigate.prefill');

      if (figure && incident.createdAt !== undefined) {
        const range = incidentRange({ createdAt: incident.createdAt, resolvedAt: incident.resolvedAt }, rule?.query?.periodSecs);
        const about = {
          application,
          environment: figure.environment ?? '',
          range: { from: Date.parse(range.from) * 1_000, to: Date.parse(range.to) * 1_000 },
          focus: {
            kind: 'incident' as const,
            summary: incident.summary,
            sessions: sessionSamples({ triggeringQuery: incident.triggeringQuery ?? null }, MOST_SESSIONS).map((sample) => sample.session),
          },
        };

        prompt = rumPrompt(rumT, (micros) => time.micros(micros, false), about);
        references.push(rumReference(about));
      }

      useMoleAgentStore.getState().start({
        prompt,
        teamId: subject.team?.id ?? null,
        subject: { kind: 'incident', id: incident.id },
        references,
      });
      onStarted?.();
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
