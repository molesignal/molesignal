/**
 * What a piece of a line of a stack is: the error's type and its message on the first line; on a
 * frame, the function and where it is (the path, the file, the line and the column), with the
 * words and marks between them. Whatever is not read as one of them is plain text.
 */
export type StackPartKind = 'type' | 'message' | 'mark' | 'function' | 'path' | 'file' | 'position' | 'text';

export interface StackPart {
  kind: StackPartKind;
  text: string;
}

/** `at fn (where)`, as V8 writes a frame, and the JVM without the space before the bracket. */
const AT_FUNCTION = /^(\s*)(at)(\s+)(.+?)(\s*\()(.+)(\))(\s*)$/;
/** `at where`: a frame V8 knows no function of. */
const AT_PLACE = /^(\s*)(at)(\s+)(\S+:\d+(?::\d+)?)(\s*)$/;
/** `fn@where`, as Firefox and Safari write a frame, the function left out when there is none. */
const AT_SIGN = /^(\s*)([^@\s]*)(@)(\S+:\d+(?::\d+)?)(\s*)$/;
/** `Type: message`, the line an error opens with, as a browser prints an uncaught one too. */
const HEADER = /^(\s*)((?:Uncaught\s+)?[A-Za-z_$][\w$.]*)(:\s)(.*)$/;
/** Where a frame is: the path up to the file, the file, and `:line:column` when it has them. */
const PLACE = /^(.*\/)?([^/]*?)(:\d+(?::\d+)?)?$/;

const part = (kind: StackPartKind, text: string): StackPart[] => (text === '' ? [] : [{ kind, text }]);

function place(where: string): StackPart[] {
  const match = PLACE.exec(where);

  if (!match) return part('file', where);

  const [, path = '', file = '', position = ''] = match;

  return [...part('path', path), ...part('file', file), ...part('position', position)];
}

function line(text: string): StackPart[] {
  const atFunction = AT_FUNCTION.exec(text);

  if (atFunction) {
    const [, indent = '', at = '', gap = '', name = '', open = '', where = '', close = '', tail = ''] = atFunction;

    return [
      ...part('text', indent),
      ...part('mark', at + gap),
      ...part('function', name),
      ...part('mark', open),
      ...place(where),
      ...part('mark', close),
      ...part('text', tail),
    ];
  }

  const atPlace = AT_PLACE.exec(text);

  if (atPlace) {
    const [, indent = '', at = '', gap = '', where = '', tail = ''] = atPlace;

    return [...part('text', indent), ...part('mark', at + gap), ...place(where), ...part('text', tail)];
  }

  const atSign = AT_SIGN.exec(text);

  if (atSign) {
    const [, indent = '', name = '', sign = '', where = '', tail = ''] = atSign;

    return [
      ...part('text', indent),
      ...part('function', name),
      ...part('mark', sign),
      ...place(where),
      ...part('text', tail),
    ];
  }

  const header = HEADER.exec(text);

  if (header) {
    const [, indent = '', type = '', colon = '', message = ''] = header;

    return [...part('text', indent), ...part('type', type), ...part('mark', colon), ...part('message', message)];
  }

  return part('text', text);
}

/**
 * A stack as lines of pieces to colour. The pieces of a line put together are the line itself, so
 * what is copied off the page is the stack as it came.
 */
export function stackLines(stack: string): StackPart[][] {
  return stack.split('\n').map(line);
}
