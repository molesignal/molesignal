import type { Editor } from '@tiptap/core';
import { Fragment, type Schema, Slice } from '@tiptap/pm/model';

/** A character that is neither space nor punctuation: one inside a word. */
const WORD = /[^\s\p{P}\p{S}]/u;
/** What a backslash escapes in Markdown, but `/`: `\/` is how a question that starts with a slash says it is no command. */
const ESCAPABLE = /[!-.:-@[-`{-~]/;

/**
 * Plain text written as Markdown that reads as it was typed. A mark is escaped
 * only where it would be read as Markdown: `checkout_api`, `a > b`, `R&D` and
 * `2 * 3` stay as they are, while `*bold*`, `` `code` ``, `[a](b)`, `~x~`,
 * `<tag>` and `&amp;` keep their backslash.
 */
export function naturalEscape(text: string): string {
  const pairedTildes = (text.match(/~/g) ?? []).length >= 2;
  const linkLike = /\[[^\]]*\][([:]/.test(text);
  let out = '';
  for (let index = 0; index < text.length; index++) {
    const char = text.charAt(index);
    const before = text.charAt(index - 1);
    const after = text.charAt(index + 1);
    switch (char) {
      case '\\':
        out += ESCAPABLE.test(after) ? '\\\\' : '\\';
        break;
      case '`':
        out += '\\`';
        break;
      case '*':
        // Only a star between two spaces can neither open nor close emphasis.
        out += /\s/.test(before) && /\s/.test(after) ? '*' : '\\*';
        break;
      case '_':
        // Inside a word an underscore never opens or closes emphasis.
        out += WORD.test(before) && WORD.test(after) ? '_' : '\\_';
        break;
      case '[':
      case ']':
        out += linkLike ? `\\${char}` : char;
        break;
      case '~':
        out += pairedTildes ? '\\~' : '~';
        break;
      case '<':
        out += /[A-Za-z/!?]/.test(after) ? '\\<' : '<';
        break;
      case '&':
        out += /^&#?[A-Za-z0-9]+;/.test(text.slice(index)) ? '\\&' : '&';
        break;
      default:
        out += char;
    }
  }
  return out;
}

interface TextNodeJson {
  marks?: Array<string | { type: string }>;
}

interface MarkdownManagerInternals {
  codeTypes: Set<string>;
  encodeTextForMarkdown: (text: string, node: TextNodeJson, parent?: { type?: string }) => string;
}

/**
 * Has the editor write plain text the natural way above. Its own way escapes
 * every mark that could be Markdown and turns `<`, `>` and `&` into entities,
 * which reads badly in a chat, a memory or a runbook. Text in code is kept as is.
 */
export function writeNaturalMarkdown(editor: Editor): void {
  const manager = editor.markdown as unknown as MarkdownManagerInternals | undefined;
  if (!manager) return;
  manager.encodeTextForMarkdown = (text, node, parent) => {
    const inCode =
      (parent?.type != null && manager.codeTypes.has(parent.type)) ||
      (node.marks ?? []).some((mark) =>
        manager.codeTypes.has(typeof mark === 'string' ? mark : mark.type),
      );
    return inCode ? text : naturalEscape(text);
  };
}

/**
 * Pasted text read as Markdown, so a list or a heading copied from a document
 * comes in as one; pasted as plain text (with Shift), it is kept as typed.
 */
export function markdownPaste(getEditor: () => Editor | null) {
  return (text: string, _context: unknown, plain: boolean): Slice => {
    const editor = getEditor();
    if (!editor) return Slice.empty;
    const schema: Schema = editor.schema;
    if (plain || !editor.markdown) {
      const paragraph = schema.nodes.paragraph!.create(null, text ? schema.text(text) : null);
      return Slice.maxOpen(Fragment.from(paragraph));
    }
    const doc = schema.nodeFromJSON(editor.markdown.parse(text));
    return Slice.maxOpen(doc.content);
  };
}
