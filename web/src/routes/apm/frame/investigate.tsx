import type { TFunction } from 'i18next';
import { Bot } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { useTimeFormatter } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';

/** What a chat started from APM is about: a service in its environment over a range, and what on it. */
export interface AgentSubject {
  service: { name: string; environment: string };
  range: { from: Date; to: Date };
  endpoint?: string | null;
  error?: { type: string; fingerprint: string } | null;
  /** A finding already said, such as an insight's sentence. */
  finding?: string | null;
}

/** The first question of a chat started from APM: what to look into, for the person to read and send. */
export function agentPrompt(t: TFunction<'apm'>, stamp: (micros: number) => string, subject: AgentSubject): string {
  const values = {
    service: subject.service.name,
    environment: subject.service.environment,
    from: stamp(subject.range.from.getTime() * 1_000),
    to: stamp(subject.range.to.getTime() * 1_000),
  };

  if (subject.finding) return t('agent.prompt.finding', { ...values, finding: subject.finding });
  if (subject.error) return t('agent.prompt.error', { ...values, error: subject.error.type, fingerprint: subject.error.fingerprint });
  if (subject.endpoint) return t('agent.prompt.endpoint', { ...values, endpoint: subject.endpoint });
  return t('agent.prompt.service', values);
}

/**
 * Investigate with Mole Agent: a new chat about the service, which it
 * investigates, with the service attached and a first question to read and
 * send. Shown to whoever may use Mole Agent.
 */
export function InvestigateButton({ subject, compact = false }: { subject: AgentSubject; compact?: boolean }) {
  const { t } = useTranslation('apm');
  const time = useTimeFormatter();
  const access = useActionAccess({ permission: 'agent.use' });

  if (!access.allowed) return null;

  const start = () =>
    useMoleAgentStore.getState().start({
      prompt: agentPrompt(t, (micros) => time.micros(micros, false), subject),
      subject: { kind: 'service', id: subject.service.name },
      references: [{ kind: 'service', id: subject.service.name, label: subject.service.name }],
    });

  return (
    <button
      type="button"
      onClick={start}
      title={t('agent.investigate_title')}
      data-apm-investigate=""
      className={cn(
        'inline-flex shrink-0 items-center gap-1.5 rounded-md font-sans text-xs font-strong outline-none',
        compact
          ? 'h-6 px-1.5 text-indigo-soft hover:bg-bg-2 focus-visible:bg-bg-2'
          : 'h-7 bg-[var(--control-surface)] px-2 text-tx-1 hover:bg-bg-3 focus-visible:bg-bg-3',
      )}
    >
      <Bot aria-hidden className={cn('h-3.5 w-3.5', compact ? 'text-indigo-soft' : 'text-tx-3')} />
      {t('agent.investigate')}
    </button>
  );
}
