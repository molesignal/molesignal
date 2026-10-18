import type { Editor } from '@tiptap/core';
import { TaskItem, TaskList } from '@tiptap/extension-list';
import { TableKit } from '@tiptap/extension-table';
import { Placeholder } from '@tiptap/extensions';
import { Markdown } from '@tiptap/markdown';
import { EditorContent, useEditor } from '@tiptap/react';
import StarterKit from '@tiptap/starter-kit';
import * as React from 'react';

import { markdownPaste, writeNaturalMarkdown } from '../../markdownEditor/naturalMarkdown';

import '../../markdownEditor/markdownEditor.css';

/** What the composer holds, and where its caret is. */
export interface ComposerText {
  /** As Markdown: what is sent. */
  markdown: string;
  /** As plain text, which the menus of commands and of things to attach read. */
  text: string;
  /** The caret's place in `text`. */
  caret: number;
}

/** What the page and the menus do to the composer from outside. */
export interface ComposerHandle {
  /** The focus in the composer, the caret at the end of the text. */
  focus: () => void;
  /** Takes out the `count` characters before the caret (an `@word`) and one space after it. */
  deleteBeforeCaret: (count: number) => void;
}

/** A key the composer is about to handle, and whether the caret is in a list or in code. */
export type ComposerKeyHandler = (event: KeyboardEvent, where: { structured: boolean }) => boolean;

function read(editor: Editor): ComposerText {
  const { doc, selection } = editor.state;
  return {
    markdown: editor.getMarkdown(),
    text: editor.getText({ blockSeparator: '\n' }),
    caret: doc.textBetween(0, selection.from, '\n').length,
  };
}

/**
 * Where the question is written, as it will read: "- " turns into a bullet,
 * "1. " into a number, "```" into code. Enter sends; in a list or in code it
 * goes on with the list or the code (Enter on an empty item leaves the list),
 * and ⌘/Ctrl+Enter sends from anywhere. Shift+Enter starts a new line, where a
 * list can begin. What it hands back is Markdown.
 */
export const ComposerEditor = React.forwardRef<
  ComposerHandle,
  {
    value: string;
    onChange: (next: ComposerText) => void;
    /** The caret moved without the text changing. */
    onCaret: (next: ComposerText) => void;
    /** True when the key was taken (a menu's, or the one that sends). */
    onKeyDown: ComposerKeyHandler;
    ariaLabel: string;
    placeholder: string;
    /** The menu over the composer: open, which one, and which of its rows is active. */
    expanded: boolean;
    controls?: string | undefined;
    activeDescendant?: string | undefined;
  }
>(function ComposerEditor(
  { value, onChange, onCaret, onKeyDown, ariaLabel, placeholder, expanded, controls, activeDescendant },
  ref,
) {
  // The Markdown last handed out, so a value coming back is not set again under the caret.
  const written = React.useRef(value);
  const handlers = React.useRef({ onChange, onCaret, onKeyDown });
  handlers.current = { onChange, onCaret, onKeyDown };
  const current = React.useRef<Editor | null>(null);

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        underline: false,
        link: { openOnClick: false, autolink: true },
        // No empty line kept at the end: a question ends where it is written.
        trailingNode: false,
      }),
      TaskList,
      TaskItem.configure({ nested: true }),
      TableKit.configure({ table: { resizable: false } }),
      Placeholder.configure({ placeholder }),
      Markdown,
    ],
    content: value,
    contentType: 'markdown',
    onCreate: ({ editor: created }) => writeNaturalMarkdown(created),
    onUpdate: ({ editor: changed }) => {
      const next = read(changed);
      written.current = next.markdown;
      handlers.current.onChange(next);
    },
    onSelectionUpdate: ({ editor: moved }) => handlers.current.onCaret(read(moved)),
    editorProps: {
      clipboardTextParser: markdownPaste(() => current.current),
      attributes: {
        role: 'combobox',
        'aria-autocomplete': 'list',
        'data-testid': 'composer-input',
        'aria-label': ariaLabel,
        spellcheck: 'false',
        // The type is the field's (on the box around it); the text scrolls past 200px.
        class: 'block max-h-[200px] overflow-y-auto py-3 pl-3.5 pr-1 outline-none',
      },
      handleKeyDown: (_view, event) => {
        const active = current.current;
        if (!active) return false;
        // A character being put together by an input method is not a key of the composer's.
        if (event.isComposing || event.keyCode === 229) return false;
        const structured =
          active.isActive('codeBlock') || active.isActive('listItem') || active.isActive('taskItem');
        if (handlers.current.onKeyDown(event, { structured })) return true;
        // Shift+Enter starts a new line of its own, where "- " or "1. " starts a list; in a
        // list or in code it breaks the line where the caret is.
        if (event.key === 'Enter' && event.shiftKey && !structured) {
          return active.commands.splitBlock();
        }
        return false;
      },
    },
  });
  current.current = editor;

  // A value set from outside (a starter, `/help`, a question given back, the text sent and
  // cleared) replaces what is shown, with the caret at its end.
  React.useEffect(() => {
    if (!editor || value === written.current) return;
    written.current = value;
    editor.commands.setContent(value, { contentType: 'markdown', emitUpdate: false });
    editor.commands.setTextSelection(editor.state.doc.content.size);
    handlers.current.onCaret(read(editor));
  }, [editor, value]);

  // The menu over the composer is named on the field, as a combobox's popup.
  React.useEffect(() => {
    const dom = editor?.view.dom;
    if (!dom) return;
    dom.setAttribute('aria-expanded', String(expanded));
    if (controls) dom.setAttribute('aria-controls', controls);
    else dom.removeAttribute('aria-controls');
    if (activeDescendant) dom.setAttribute('aria-activedescendant', activeDescendant);
    else dom.removeAttribute('aria-activedescendant');
  }, [editor, expanded, controls, activeDescendant]);

  React.useImperativeHandle(
    ref,
    () => ({
      focus: () => {
        editor?.commands.focus('end');
      },
      deleteBeforeCaret: (count) => {
        if (!editor) return;
        const { from } = editor.state.selection;
        const after = editor.state.doc.textBetween(from, Math.min(from + 1, editor.state.doc.content.size));
        editor
          .chain()
          .focus()
          .deleteRange({ from: Math.max(0, from - count), to: after === ' ' ? from + 1 : from })
          .run();
      },
    }),
    [editor],
  );

  return (
    <div
      data-agent-composer-editor
      className="md-editor md-editor--inherit min-h-12 min-w-0 flex-1 cursor-text font-sans text-base leading-6 text-tx-0 md:text-sm"
    >
      <EditorContent editor={editor} />
    </div>
  );
});
