import * as monaco from 'monaco-editor/editor/editor.api.js';
import 'monaco-editor/language/json/monaco.contribution.js';
import 'monaco-editor/languages/definitions/dart/register.js';
import 'monaco-editor/languages/definitions/go/register.js';
import 'monaco-editor/languages/definitions/ini/register.js';
import 'monaco-editor/languages/definitions/kotlin/register.js';
import 'monaco-editor/languages/definitions/powershell/register.js';
import 'monaco-editor/languages/definitions/python/register.js';
import 'monaco-editor/languages/definitions/rust/register.js';
import 'monaco-editor/languages/definitions/shell/register.js';
import 'monaco-editor/languages/definitions/sql/register.js';
import 'monaco-editor/languages/definitions/swift/register.js';
import 'monaco-editor/languages/definitions/typescript/register.js';
import 'monaco-editor/languages/definitions/yaml/register.js';

export type HighlightTokenKind =
  | 'plain'
  | 'comment'
  | 'keyword'
  | 'string'
  | 'number'
  | 'type'
  | 'function'
  | 'property'
  | 'operator'
  | 'punctuation'
  | 'identifier'
  | 'invalid';

export interface HighlightToken {
  text: string;
  kind: HighlightTokenKind;
}

export type HighlightedLine = HighlightToken[];

const LANGUAGE_ID: Record<string, string> = {
  bash: 'shell',
  sh: 'shell',
  shell: 'shell',
  ts: 'typescript',
  typescript: 'typescript',
  js: 'javascript',
  javascript: 'javascript',
  toml: 'ini',
  text: 'plaintext',
  plaintext: 'plaintext',
};

export async function highlightCode(
  content: string,
  language: string,
): Promise<HighlightedLine[]> {
  const languageId = LANGUAGE_ID[language.toLocaleLowerCase()] ?? language.toLocaleLowerCase();

  // `colorize` waits for Monaco's lazy Monarch language definition. Once it
  // resolves, `tokenize` exposes semantic token names without rendering an
  // editor or trusting generated HTML.
  await monaco.editor.colorize(content, languageId, { tabSize: 2 });
  const tokenLines = monaco.editor.tokenize(content, languageId);
  const sourceLines = content.split('\n');

  return sourceLines.map((line, lineIndex) => {
    const tokens = tokenLines[lineIndex] ?? [];
    if (tokens.length === 0) {
      return [{ text: line, kind: 'plain' }];
    }
    return tokens.map((token, tokenIndex) => ({
      text: line.slice(
        token.offset,
        tokens[tokenIndex + 1]?.offset ?? line.length,
      ),
      kind: classifyToken(token.type),
    }));
  });
}

function classifyToken(type: string): HighlightTokenKind {
  const normalized = type.toLocaleLowerCase();
  if (normalized.includes('invalid')) return 'invalid';
  if (normalized.includes('comment')) return 'comment';
  if (normalized.includes('string') || normalized.includes('regexp')) return 'string';
  if (normalized.includes('number')) return 'number';
  if (normalized.includes('keyword') || normalized.includes('predefined')) return 'keyword';
  if (normalized.includes('function') || normalized.includes('method')) return 'function';
  if (
    normalized.includes('type') ||
    normalized.includes('class') ||
    normalized.includes('namespace')
  ) {
    return 'type';
  }
  if (normalized.includes('attribute') || normalized.includes('property')) return 'property';
  if (normalized.includes('operator')) return 'operator';
  if (normalized.includes('delimiter') || normalized.includes('bracket')) return 'punctuation';
  if (
    normalized.includes('identifier') ||
    normalized.includes('variable') ||
    normalized.includes('tag')
  ) {
    return 'identifier';
  }
  return 'plain';
}
