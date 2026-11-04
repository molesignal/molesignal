import { Bot } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { useTimeFormatter } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';

import { type RumAgentSubject, rumPrompt, rumReference } from './prompt';
import { HEAD_ACTION } from '../headActions';

/** How the entry looks where it stands: in a page's head, as a short label in a drawer, or as an icon in a row. */
export type InvestigateStyle = 'head' | 'label' | 'icon';

/**
 * Investigate with Mole Agent: a new chat about the application, with the application attached and
 * a first question drafted in the input for the person to read, change and send. It is never sent
 * for them. Shown to whoever may use Mole Agent, and to nobody else.
 */
export function RumInvestigateButton({
  subject,
  look = 'head',
  label,
  title,
  className,
  onStarted,
}: {
  subject: RumAgentSubject;
  look?: InvestigateStyle;
  /** What the entry says; the whole name of the entry in a page's head. */
  label?: string | undefined;
  /** What the entry answers to a hover and to a screen reader; the label by default. */
  title?: string | undefined;
  className?: string | undefined;
  /** What to do once the chat is started: a drawer over the panel closes, so the question can be reached. */
  onStarted?: (() => void) | undefined;
}) {
  const { t } = useTranslation('rum');
  const time = useTimeFormatter();
  const access = useActionAccess({ permission: 'agent.use' });

  if (!access.allowed) return null;

  const text = label ?? t('agent.investigate');

  const start = () => {
    useMoleAgentStore.getState().start({
      prompt: rumPrompt(t, (micros) => time.micros(micros, false), subject),
      subject: { kind: 'rum', id: subject.application.id },
      references: [rumReference(subject)],
    });
    onStarted?.();
  };

  return (
    <button
      type="button"
      onClick={start}
      title={title ?? text}
      aria-label={look === 'icon' ? (title ?? text) : undefined}
      data-rum-investigate={subject.focus?.kind ?? 'overview'}
      className={cn(
        'inline-flex shrink-0 items-center gap-1.5 rounded-md border-0 font-sans font-strong outline-none transition-colors',
        look === 'head' && HEAD_ACTION,
        look === 'label' &&
          'h-8 bg-[var(--control-surface)] px-2.5 text-xs text-tx-1 hover:bg-bg-3 focus-visible:bg-bg-3',
        look === 'icon' &&
          "relative z-10 h-6 w-6 justify-center bg-transparent p-0 text-tx-3 after:absolute after:-inset-1 after:content-[''] hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0",
        className,
      )}
    >
      <Bot aria-hidden className={cn('h-3.5 w-3.5', look === 'head' && 'text-tx-3')} />
      {look !== 'icon' && text}
    </button>
  );
}
