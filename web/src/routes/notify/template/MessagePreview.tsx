import DOMPurify from 'dompurify';

import type { NotifyTemplate } from '@/api/notify/templates';
import { MarkdownMessage } from '@/routes/intelligence/markdown';

type TemplateFormat = NonNullable<NotifyTemplate['format']>;

export function TemplateMessagePreview({
  format,
  content,
  emptyText,
}: {
  format: TemplateFormat;
  content: string;
  emptyText: string;
}) {
  if (!content) {
    return <p className="mt-3 text-xs text-tx-3">{emptyText}</p>;
  }

  if (format === 'markdown') {
    return (
      <MarkdownMessage
        content={content}
        className="mt-3 max-h-72 overflow-auto text-sm"
      />
    );
  }

  if (format === 'html') {
    return (
      <div
        className="mt-3 max-h-72 overflow-auto break-words text-sm leading-relaxed text-tx-1 [&_a]:text-indigo [&_blockquote]:border-l-2 [&_blockquote]:border-indigo/50 [&_blockquote]:pl-3 [&_code]:rounded [&_code]:bg-bg-3 [&_code]:px-1 [&_code]:py-0.5 [&_code]:font-mono [&_h1]:mb-2 [&_h1]:text-lg [&_h1]:font-display-strong [&_h2]:mb-2 [&_h2]:text-base [&_h2]:font-display-strong [&_h3]:mb-2 [&_h3]:text-sm [&_h3]:font-display-strong [&_li]:ml-5 [&_ol]:my-2 [&_ol]:list-decimal [&_p]:my-2 [&_pre]:my-2 [&_pre]:whitespace-pre-wrap [&_pre]:rounded-md [&_pre]:bg-bg-3 [&_pre]:p-3 [&_ul]:my-2 [&_ul]:list-disc"
        dangerouslySetInnerHTML={{
          __html: sanitizeHtmlPreview(content),
        }}
      />
    );
  }

  return (
    <pre className="mt-3 max-h-72 overflow-auto whitespace-pre-wrap break-words font-mono text-xs leading-relaxed text-tx-1">
      {content}
    </pre>
  );
}

export function sanitizeHtmlPreview(content: string): string {
  return DOMPurify.sanitize(content, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: [
      'button',
      'embed',
      'form',
      'iframe',
      'input',
      'object',
      'script',
      'select',
      'style',
      'textarea',
    ],
    FORBID_ATTR: ['style'],
  });
}
