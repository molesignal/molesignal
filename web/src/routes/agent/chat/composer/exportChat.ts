/**
 * `/export`: the chat as a Markdown file, the way it reads on screen. Answers
 * written as a structured result keep their sections.
 */
import type { TFunction } from 'i18next';

import type { ChatMessageRow } from '@/api/agent/chat';

import { displayText } from './commands';
import { parseStructuredAnswer } from '../../answer';
import { visibleChatMessages } from '../../messageRenderState';
import { sanitizeAssistantContent } from '../../settings/presentation';

export interface ExportLabels {
  you: string;
  agent: string;
  conclusion: string;
  evidence: string;
  likelyCauses: string;
  limitations: string;
  nextSteps: string;
}

/** The headings of an answer written out, in the reader's language. */
export function exportLabels(t: TFunction<'agent'>): ExportLabels {
  return {
    you: t('commands.export_you'),
    agent: 'Mole Agent',
    conclusion: t('answer.conclusion'),
    evidence: t('answer.evidence'),
    likelyCauses: t('answer.likely_causes'),
    limitations: t('answer.limitations'),
    nextSteps: t('answer.next_steps'),
  };
}

function list(title: string, items: readonly string[] | undefined): string[] {
  if (!items || items.length === 0) return [];
  return [`**${title}**`, '', ...items.map((item) => `- ${item}`), ''];
}

/** An answer as Markdown: a structured result keeps its sections. */
export function answerMarkdown(content: string, labels: ExportLabels): string {
  const clean = sanitizeAssistantContent(content);
  const answer = parseStructuredAnswer(clean);
  if (!answer) return clean;
  const lines = [
    ...(answer.summary ? [`**${labels.conclusion}**`, '', answer.summary, ''] : []),
    ...list(
      labels.evidence,
      answer.evidence?.map((item) => item.label ?? '').filter(Boolean),
    ),
    ...list(labels.likelyCauses, answer.likely_causes),
    ...list(labels.limitations, answer.limitations),
    ...list(labels.nextSteps, answer.suggested_next_steps),
  ];
  return lines.join('\n').trim();
}

/** UTC, to the minute, the same for whoever opens the file. */
function when(micros: number): string {
  return new Date(micros / 1000).toISOString().slice(0, 16).replace('T', ' ') + ' UTC';
}

export function chatMarkdown(
  title: string,
  messages: readonly ChatMessageRow[],
  labels: ExportLabels,
): string {
  const parts = [`# ${title}`];
  for (const message of visibleChatMessages([...messages])) {
    if (message.role !== 'user' && message.role !== 'assistant') continue;
    const who = message.role === 'user' ? labels.you : labels.agent;
    const body =
      message.role === 'user'
        ? displayText(message.content)
        : answerMarkdown(message.content, labels);
    parts.push(`## ${who} · ${when(message.created_at_micros)}`, body);
  }
  return `${parts.join('\n\n')}\n`;
}

/** A file name from the chat's title, safe on every system. */
export function exportFileName(title: string, nowMs: number): string {
  const stem =
    title
      .trim()
      .replace(/[\\/:*?"<>|\s]+/g, '-')
      .replace(/-+/g, '-')
      .replace(/^-|-$/g, '')
      .slice(0, 60) || 'mole-agent-chat';
  return `${stem}-${new Date(nowMs).toISOString().slice(0, 10)}.md`;
}

export function downloadText(fileName: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: 'text/markdown;charset=utf-8' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = fileName;
  document.body.append(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}
