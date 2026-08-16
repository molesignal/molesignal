import type { FunctionLanguage } from '@/api/functions';

export const DEFAULT_VRL_SOURCE = `# VRL transform — receives event in \`.\`, returns the modified event.
.environment = "production"
.timestamp = now()`;

export const DEFAULT_JS_SOURCE = `// JavaScript transform — use the built-in molesignal event API.
molesignal.set('environment', 'production');`;

const LEGACY_DEFAULT_JS_SOURCE = `// JS transform — receives event, returns the modified event.
export default function transform(event) {
  return { ...event, environment: 'production' };
}`;

export function defaultFunctionSource(language: FunctionLanguage): string {
  return language === 'js' ? DEFAULT_JS_SOURCE : DEFAULT_VRL_SOURCE;
}

export function normalizeLoadedFunctionSource(
  language: FunctionLanguage,
  source: string,
): string {
  if (language !== 'js') return source;
  const normalized = source.replace(/\r\n?/g, '\n').trim();
  return normalized === LEGACY_DEFAULT_JS_SOURCE ? DEFAULT_JS_SOURCE : source;
}
