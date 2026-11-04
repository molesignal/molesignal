import type { TFunction } from 'i18next';

import type { MessageReference } from '@/api/agent/chat';

/** The longest an error message is repeated in a question. */
const MESSAGE_CHARS = 160;

/** What on an application a chat started from RUM looks into. None is the application as a whole. */
export type RumFocus =
  | { kind: 'errors' }
  | { kind: 'performance' }
  | { kind: 'frustration' }
  /** One Core Web Vital: its key as the performance page names it (`lcp`, `inp`, `cls`…). */
  | { kind: 'vital'; vital: string }
  | { kind: 'issue'; id: string; type: string; message: string }
  /** The views that have one value of a dimension: a page, a browser, a country, a version. */
  | { kind: 'slice'; dimension: string; value: string; vital?: string; figure?: string }
  | { kind: 'element'; name: string; selector: string | null }
  | { kind: 'session'; id: string }
  /** An alert of a rule on a figure of the application, with the sessions it was told by. */
  | { kind: 'incident'; summary: string; sessions: readonly string[] };

/** An application in its environment over a range, and what on it a chat is about. */
export interface RumAgentSubject {
  application: { id: string; name: string };
  /** Empty for every environment. */
  environment: string;
  /** Microseconds. */
  range: { from: number; to: number };
  focus?: RumFocus | undefined;
}

const cut = (text: string): string => (text.length > MESSAGE_CHARS ? `${text.slice(0, MESSAGE_CHARS)}…` : text);

/**
 * The first question of a chat started from RUM: what to look into, for the person to read, change
 * and send. The application, the environment and the range are written into it, so the model has
 * nothing to guess.
 */
export function rumPrompt(t: TFunction<'rum'>, stamp: (micros: number) => string, subject: RumAgentSubject): string {
  const focus = subject.focus;
  const values = {
    application: subject.application.name,
    environment: subject.environment || t('agent.all_environments'),
    from: stamp(subject.range.from),
    to: stamp(subject.range.to),
  };

  switch (focus?.kind) {
    case 'errors':
    case 'performance':
    case 'frustration':
      return t(`agent.prompt.${focus.kind}`, values);
    case 'vital':
      return t('agent.prompt.vital', { ...values, vital: focus.vital.toUpperCase() });
    case 'issue':
      return t('agent.prompt.issue', {
        ...values,
        issue: focus.id,
        error: cut([focus.type, focus.message].filter(Boolean).join(': ')),
      });
    case 'slice': {
      const slice = { ...values, dimension: focus.dimension, value: focus.value };

      return focus.vital && focus.figure
        ? t('agent.prompt.slice_figure', { ...slice, vital: focus.vital.toUpperCase(), figure: focus.figure })
        : t('agent.prompt.slice', slice);
    }
    case 'element':
      return t(focus.selector ? 'agent.prompt.element_selector' : 'agent.prompt.element', {
        ...values,
        element: cut(focus.name),
        selector: focus.selector ?? '',
      });
    case 'session':
      return t('agent.prompt.session', { ...values, session: focus.id });
    case 'incident':
      return t(focus.sessions.length > 0 ? 'agent.prompt.incident_sessions' : 'agent.prompt.incident', {
        ...values,
        summary: cut(focus.summary),
        sessions: focus.sessions.join(', '),
      });
    default:
      return t('agent.prompt.overview', values);
  }
}

/** The application, attached to the question for the model to read as its subject. */
export function rumReference(subject: RumAgentSubject): MessageReference {
  return { kind: 'rum_application', id: subject.application.id, label: subject.application.name };
}
