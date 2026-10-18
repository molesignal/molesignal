import { Editor, type Extensions } from '@tiptap/core';
import { TaskItem, TaskList } from '@tiptap/extension-list';
import { TableKit } from '@tiptap/extension-table';
import { Placeholder } from '@tiptap/extensions';
import { Markdown } from '@tiptap/markdown';
import { EditorContent, useEditor } from '@tiptap/react';
import StarterKit from '@tiptap/starter-kit';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { markdownPaste, writeNaturalMarkdown } from './naturalMarkdown';

import './markdownEditor.css';

/** What the editor knows of Markdown: CommonMark with GitHub's task lists and tables. */
function markdownExtensions(placeholder = ''): Extensions {
  return [
    StarterKit.configure({
      // Underline has no Markdown of its own.
      underline: false,
      link: { openOnClick: false, autolink: true },
    }),
    TaskList,
    TaskItem.configure({ nested: true }),
    TableKit.configure({ table: { resizable: false } }),
    Placeholder.configure({ placeholder }),
    Markdown,
  ];
}

/** The text a piece of Markdown says, without its marks and spacing. */
function wordsOf(markdown: string): string {
  return markdown.replace(/[\s*_`#>|~[\]()!\\:+\-.=]/g, '');
}

/**
 * Whether the live editor would lose some of this Markdown when it writes it
 * back: an HTML comment, a footnote, raw HTML it does not know. Such a text is
 * changed as source instead. A list marker or a heading style it writes its own
 * way is not a loss.
 */
export function liveEditingLoses(markdown: string): boolean {
  const editor = new Editor({ extensions: markdownExtensions(), content: markdown, contentType: 'markdown' });
  writeNaturalMarkdown(editor);
  try {
    return wordsOf(editor.getMarkdown()) !== wordsOf(markdown);
  } finally {
    editor.destroy();
  }
}

/**
 * A Markdown field that reads as it will be shown while it is written: "- "
 * turns into a bullet, "# " into a heading, "[ ] " into a task. What it hands
 * back is Markdown. It reports only what the person changes: opening a text
 * does not change it.
 */
export function MarkdownEditor({
  value,
  onChange,
  ariaLabel,
  placeholder,
  className,
  autoFocus = false,
}: {
  value: string;
  onChange: (markdown: string) => void;
  ariaLabel: string;
  placeholder?: string;
  /** The field's box: its height, fill and padding. */
  className?: string;
  autoFocus?: boolean;
}) {
  // The Markdown last handed out, so a value coming back is not set again under the caret.
  const written = React.useRef(value);
  const report = React.useRef(onChange);
  report.current = onChange;
  const current = React.useRef<Editor | null>(null);

  const editor = useEditor({
    extensions: markdownExtensions(placeholder),
    content: value,
    contentType: 'markdown',
    autofocus: autoFocus ? 'end' : false,
    onCreate: ({ editor: created }) => writeNaturalMarkdown(created),
    editorProps: {
      clipboardTextParser: markdownPaste(() => current.current),
      attributes: {
        'aria-label': ariaLabel,
        'aria-multiline': 'true',
        role: 'textbox',
        spellcheck: 'false',
      },
    },
    onUpdate: ({ editor: current }) => {
      const markdown = current.getMarkdown();
      written.current = markdown;
      report.current(markdown);
    },
  });

  current.current = editor;

  // A value set from outside (a draft given up, another file opened) replaces what is shown.
  React.useEffect(() => {
    if (!editor || value === written.current) return;
    written.current = value;
    editor.commands.setContent(value, { contentType: 'markdown', emitUpdate: false });
  }, [editor, value]);

  return (
    <div
      data-markdown-editor
      className={cn('md-editor cursor-text', className)}
      onMouseDown={(event) => {
        // A click below the last line still puts the caret in the text.
        if (event.target === event.currentTarget) {
          event.preventDefault();
          editor?.commands.focus('end');
        }
      }}
    >
      <EditorContent editor={editor} className="min-h-[inherit]" />
    </div>
  );
}
