import { Editor } from '@tiptap/core';
import { Markdown } from '@tiptap/markdown';
import StarterKit from '@tiptap/starter-kit';
import { afterEach, describe, expect, it } from 'vitest';

import { markdownPaste, naturalEscape, writeNaturalMarkdown } from './naturalMarkdown';

const editors: Editor[] = [];
afterEach(() => editors.splice(0).forEach((editor) => editor.destroy()));

function editor(): Editor {
  const created = new Editor({ extensions: [StarterKit, Markdown] });
  writeNaturalMarkdown(created);
  editors.push(created);
  return created;
}

/** Plain text typed into the editor, written out, and read back in. */
function roundTrip(text: string): { markdown: string; back: string } {
  const first = editor();
  first.commands.setContent({
    type: 'doc',
    content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
  });
  const markdown = first.getMarkdown();
  const second = editor();
  second.commands.setContent(markdown, { contentType: 'markdown' });
  return { markdown, back: second.getText() };
}

describe('naturalEscape', () => {
  it('leaves what cannot be read as Markdown as it was typed', () => {
    expect(naturalEscape('checkout_api error_rate > 5% for R&D, 2 * 3')).toBe(
      'checkout_api error_rate > 5% for R&D, 2 * 3',
    );
    expect(naturalEscape('看 a < b，[x] 待办，~5 分钟')).toBe('看 a < b，[x] 待办，~5 分钟');
    expect(naturalEscape('C:\\path')).toBe('C:\\path');
    // A question that starts with a slash and is no command keeps its one backslash.
    expect(naturalEscape('\\/etc/hosts changed?')).toBe('\\/etc/hosts changed?');
  });

  it('escapes what would be read as Markdown', () => {
    expect(naturalEscape('*all* pods')).toBe('\\*all\\* pods');
    expect(naturalEscape('_so_ slow')).toBe('\\_so\\_ slow');
    expect(naturalEscape('use `kubectl`')).toBe('use \\`kubectl\\`');
    expect(naturalEscape('[a](b)')).toBe('\\[a\\](b)');
    expect(naturalEscape('~old~ new')).toBe('\\~old\\~ new');
    expect(naturalEscape('<service> and &amp;')).toBe('\\<service> and \\&amp;');
    expect(naturalEscape('a\\*b')).toBe('a\\\\\\*b');
  });
});

describe('writeNaturalMarkdown', () => {
  it.each([
    'checkout_api error_rate > 5% for R&D',
    '*all* pods, a*b and 2 * 3',
    '_so_ slow, snake_case_name',
    'use `kubectl` and [a](b) or [x] alone',
    '~old~ new and ~5 minutes',
    '<service> placeholder, a < b, &amp; and &',
    'C:\\path and a\\*b',
    '交易 API → 撮合 → 清算（prod 上最关键）',
  ])('reads %s back as it was typed', (text) => {
    expect(roundTrip(text).back).toBe(text);
  });

  it('writes a plain sentence without backslashes or entities', () => {
    expect(roundTrip('checkout_api p99 > 2s & rising').markdown).toBe(
      'checkout_api p99 > 2s & rising',
    );
  });

  it('keeps code as it is', () => {
    const typed = editor();
    typed.commands.setContent('use `a_b*c<d>` here', { contentType: 'markdown' });
    expect(typed.getMarkdown()).toBe('use `a_b*c<d>` here');
  });
});

describe('markdownPaste', () => {
  it('reads pasted Markdown as Markdown, and Shift-pasted text as typed', () => {
    const target = editor();
    const paste = markdownPaste(() => target);

    const list = paste('- one\n- two', null, false);
    expect(list.content.firstChild?.type.name).toBe('bulletList');

    const plain = paste('- one', null, true);
    expect(plain.content.firstChild?.textContent).toBe('- one');
  });
});
