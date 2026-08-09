import * as React from 'react';
import { Link } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';

type HeadingLevel = 1 | 2 | 3 | 4;

export type MarkdownBlock =
  | { type: 'heading'; level: HeadingLevel; text: string }
  | { type: 'paragraph'; text: string }
  | { type: 'list'; ordered: boolean; items: string[] }
  | { type: 'code'; lang?: string; code: string }
  | { type: 'quote'; text: string }
  | { type: 'table'; headers: string[]; rows: string[][] };

const FENCE_RE = /^\s*```([\w-]+)?\s*$/;
const HEADING_RE = /^(#{1,4})\s+(.+)$/;
const UNORDERED_RE = /^\s*[-*+]\s+(.+)$/;
const ORDERED_RE = /^\s*\d+[.)]\s+(.+)$/;
const QUOTE_RE = /^\s*>\s?(.*)$/;

/**
 * Models occasionally append a block heading to the end of a sentence, for
 * example `已完成查询： ## 结果`. CommonMark only treats a heading marker as
 * block syntax at the start of a line, so repair that narrow case before
 * parsing. Fenced code is intentionally left byte-for-byte unchanged.
 */
export function normalizeMarkdownSource(content: string): string {
  const lines = content.replace(/\r\n/g, '\n').split('\n');
  let insideFence = false;

  return lines
    .map((line) => {
      if (FENCE_RE.test(line)) {
        insideFence = !insideFence;
        return line;
      }
      if (insideFence) return line;

      return line.replace(
        /([:：。.!?！？])\s+(#{1,4})\s+(?=\S)/g,
        '$1\n\n$2 ',
      );
    })
    .join('\n');
}

export function parseMarkdownBlocks(content: string): MarkdownBlock[] {
  const lines = normalizeMarkdownSource(content).split('\n');
  const blocks: MarkdownBlock[] = [];
  const paragraph: string[] = [];

  const flushParagraph = () => {
    const text = paragraph.join('\n').trim();
    if (text) blocks.push({ type: 'paragraph', text });
    paragraph.length = 0;
  };

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i] ?? '';
    const trimmed = line.trim();

    if (!trimmed) {
      flushParagraph();
      continue;
    }

    const fence = line.match(FENCE_RE);
    if (fence) {
      flushParagraph();
      const lang = fence[1];
      const code: string[] = [];
      i += 1;
      while (i < lines.length && !FENCE_RE.test(lines[i] ?? '')) {
        code.push(lines[i] ?? '');
        i += 1;
      }
      blocks.push(lang ? { type: 'code', lang, code: code.join('\n') } : { type: 'code', code: code.join('\n') });
      continue;
    }

    const heading = line.match(HEADING_RE);
    if (heading) {
      flushParagraph();
      const marks = heading[1] ?? '#';
      const text = heading[2] ?? '';
      blocks.push({
        type: 'heading',
        level: Math.min(marks.length, 4) as HeadingLevel,
        text: text.trim(),
      });
      continue;
    }

    if (isTableStart(lines, i)) {
      flushParagraph();
      const headers = splitTableRow(lines[i] ?? '');
      const rows: string[][] = [];
      i += 2;
      while (i < lines.length && isTableRow(lines[i] ?? '')) {
        rows.push(splitTableRow(lines[i] ?? ''));
        i += 1;
      }
      i -= 1;
      blocks.push({ type: 'table', headers, rows });
      continue;
    }

    const unordered = line.match(UNORDERED_RE);
    const ordered = line.match(ORDERED_RE);
    if (unordered || ordered) {
      flushParagraph();
      const listOrdered = Boolean(ordered);
      const items: string[] = [];
      while (i < lines.length) {
        const current = lines[i] ?? '';
        const match = listOrdered ? current.match(ORDERED_RE) : current.match(UNORDERED_RE);
        if (!match) break;
        items.push((match[1] ?? '').trim());
        i += 1;
      }
      i -= 1;
      blocks.push({ type: 'list', ordered: listOrdered, items });
      continue;
    }

    const quote = line.match(QUOTE_RE);
    if (quote) {
      flushParagraph();
      const parts = [(quote[1] ?? '').trim()];
      i += 1;
      while (i < lines.length) {
        const next = (lines[i] ?? '').match(QUOTE_RE);
        if (!next) break;
        parts.push((next[1] ?? '').trim());
        i += 1;
      }
      i -= 1;
      blocks.push({ type: 'quote', text: parts.join('\n') });
      continue;
    }

    paragraph.push(line);
  }

  flushParagraph();
  return blocks;
}

export function MarkdownMessage({
  content,
  streaming,
  className,
}: {
  content: string;
  streaming?: boolean;
  className?: string;
}) {
  const blocks = React.useMemo(() => parseMarkdownBlocks(content), [content]);
  // While streaming, the trailing block is the one still being written — give
  // it the live-text shimmer so the freshly arriving copy reads as "in flight".
  const lastIndex = blocks.length - 1;
  return (
    <div className={cn('flex flex-col gap-2.5 leading-relaxed', className)}>
      {blocks.map((block, index) =>
        renderBlock(block, index, Boolean(streaming) && index === lastIndex),
      )}
      {streaming && <span className="inline-block h-4 w-1.5 animate-pulse rounded-sm bg-indigo align-text-bottom" />}
    </div>
  );
}

function renderBlock(block: MarkdownBlock, index: number, shimmer = false): React.ReactNode {
  switch (block.type) {
    case 'heading': {
      const className = cn(
        'font-sans font-display-strong tracking-normal text-tx-0',
        block.level === 1 && 'text-lg',
        block.level === 2 && 'text-base',
        block.level >= 3 && 'text-sm',
      );
      const children = renderInline(block.text);
      if (block.level === 1) return <h1 key={index} className={className}>{children}</h1>;
      if (block.level === 2) return <h2 key={index} className={className}>{children}</h2>;
      if (block.level === 3) return <h3 key={index} className={className}>{children}</h3>;
      return <h4 key={index} className={className}>{children}</h4>;
    }
    case 'paragraph':
      return (
        <p
          key={index}
          className={cn('whitespace-pre-wrap break-words text-tx-1', shimmer && 'text-shimmer')}
        >
          {renderInline(block.text)}
        </p>
      );
    case 'list': {
      const ListTag = block.ordered ? 'ol' : 'ul';
      return (
        <ListTag
          key={index}
          className={cn(
            'space-y-1 pl-5 text-tx-1',
            block.ordered ? 'list-decimal' : 'list-disc',
          )}
        >
          {block.items.map((item, itemIndex) => (
            <li key={`${index}-${itemIndex}`} className="pl-0.5">
              {renderInline(item)}
            </li>
          ))}
        </ListTag>
      );
    }
    case 'code':
      return (
        <div key={index} className="overflow-hidden rounded-md border border-bd-0 bg-bg-2">
          {block.lang && (
            <div className="border-b border-bd-0 px-3 py-1 font-mono text-xs uppercase text-tx-3">
              {block.lang}
            </div>
          )}
          <pre className="whitespace-pre-wrap break-words p-3 font-mono text-xs leading-relaxed text-tx-0 [overflow-wrap:anywhere]">
            <code>{block.code}</code>
          </pre>
        </div>
      );
    case 'quote':
      return (
        <blockquote key={index} className="border-l-2 border-indigo/50 pl-3 text-tx-2">
          {renderInline(block.text)}
        </blockquote>
      );
    case 'table':
      return (
        <div key={index} className="overflow-x-auto rounded-md border border-bd-0">
          <table className="min-w-full border-collapse bg-bg-1 font-sans text-xs">
            <thead className="bg-bg-2 text-tx-2">
              <tr>
                {block.headers.map((header, headerIndex) => (
                  <th key={`${index}-h-${headerIndex}`} className="border-b border-bd-0 px-2.5 py-1.5 text-left font-strong">
                    {renderInline(header)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {block.rows.map((row, rowIndex) => (
                <tr key={`${index}-r-${rowIndex}`} className="border-t border-bd-0">
                  {block.headers.map((_, cellIndex) => (
                    <td key={`${index}-c-${rowIndex}-${cellIndex}`} className="px-2.5 py-1.5 text-tx-1">
                      {renderInline(row[cellIndex] ?? '')}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
  }
}

function renderInline(text: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  const tokenRe = /(\*\*([^*]+)\*\*|`([^`]+)`|\[([^\]]+)\]\(([^)\s]+)\))/g;
  let last = 0;
  let match: RegExpExecArray | null;

  while ((match = tokenRe.exec(text)) !== null) {
    if (match.index > last) nodes.push(text.slice(last, match.index));

    if (match[2]) {
      nodes.push(<strong key={match.index} className="font-display-strong text-tx-0">{match[2]}</strong>);
    } else if (match[3]) {
      nodes.push(
        <code key={match.index} className="rounded bg-bg-3 px-1 py-0.5 font-mono text-[0.92em] text-tx-0">
          {match[3]}
        </code>,
      );
    } else if (match[4] && match[5]) {
      nodes.push(renderLink(match[4], match[5], match.index));
    }

    last = match.index + match[0].length;
  }

  if (last < text.length) nodes.push(text.slice(last));
  return nodes;
}

function renderLink(label: string, href: string, key: React.Key): React.ReactNode {
  if (href.startsWith('/')) {
    return <Link key={key} to={href} className="text-indigo hover:underline">{label}</Link>;
  }
  if (/^https?:\/\//i.test(href)) {
    return (
      <a key={key} href={href} target="_blank" rel="noreferrer" className="text-indigo hover:underline">
        {label}
      </a>
    );
  }
  return `${label} (${href})`;
}

function isTableStart(lines: string[], index: number): boolean {
  const current = lines[index] ?? '';
  const next = lines[index + 1] ?? '';
  return isTableRow(current) && isTableDivider(next);
}

function isTableRow(line: string): boolean {
  return line.includes('|') && splitTableRow(line).length >= 2;
}

function isTableDivider(line: string): boolean {
  const cells = splitTableRow(line);
  return cells.length >= 2 && cells.every((cell) => /^:?-{3,}:?$/.test(cell));
}

function splitTableRow(line: string): string[] {
  return line
    .trim()
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
    .map((cell) => cell.trim());
}
