import { describe, expect, it } from 'vitest';

import {
  DEFAULT_JS_SOURCE,
  defaultFunctionSource,
  normalizeLoadedFunctionSource,
} from './defaults';

describe('function editor defaults', () => {
  it('uses the built-in molesignal API for new JavaScript functions', () => {
    expect(defaultFunctionSource('js')).toBe(DEFAULT_JS_SOURCE);
    expect(DEFAULT_JS_SOURCE).toContain("molesignal.set('environment', 'production')");
    expect(DEFAULT_JS_SOURCE).not.toContain('export default');
  });

  it('upgrades only the legacy built-in JavaScript template', () => {
    const legacy = `// JS transform — receives event, returns the modified event.
export default function transform(event) {
  return { ...event, environment: 'production' };
}`;
    expect(normalizeLoadedFunctionSource('js', legacy)).toBe(DEFAULT_JS_SOURCE);

    const custom = 'molesignal.set("level", "debug");';
    expect(normalizeLoadedFunctionSource('js', custom)).toBe(custom);
  });
});
