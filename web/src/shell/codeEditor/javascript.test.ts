import * as monaco from 'monaco-editor/editor/editor.api.js';
import { describe, expect, it } from 'vitest';

import { JAVASCRIPT_LANGUAGE_ID, registerJavaScriptLanguage } from './javascript';

describe('JavaScript Monaco language', () => {
  it('eagerly tokenizes comments, keywords, built-ins, and strings', () => {
    registerJavaScriptLanguage(monaco);

    const lines = monaco.editor.tokenize(
      `// transform
const value = molesignal.fields.level;
return "ok";`,
      JAVASCRIPT_LANGUAGE_ID,
    );
    const tokenTypes = lines.flatMap((line) => line.map((token) => token.type));

    expect(tokenTypes).toContain('comment.js');
    expect(tokenTypes).toContain('keyword.js');
    expect(tokenTypes).toContain('predefined.js');
    expect(tokenTypes).toContain('string.js');
  });
});
