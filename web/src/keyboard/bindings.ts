import type { Binding } from '@/keyboard/controller';

export interface BindingDoc {
  keys: string;
  description: string;
  category: 'navigation' | 'time' | 'investigation' | 'editing' | 'general';
}

/**
 * The canonical keymap documented to the user. Each entry corresponds to a
 * concrete handler wired in by the corresponding feature module — the
 * functional handler is registered at runtime via `useBindings`. This module
 * exists purely so the help overlay and `docs/web/keyboard.md` build script
 * agree on the surface.
 */
export const GLOBAL_KEYMAP: BindingDoc[] = [
  { keys: 'mod+k',       description: 'Open command palette',                  category: 'general' },
  { keys: 'mod+esc',     description: 'Pop current scope / dismiss overlay',   category: 'general' },
  { keys: 'mod+/',       description: 'Show keyboard help',                    category: 'general' },
  { keys: 'mod+[',       description: 'Investigation stack: back',             category: 'investigation' },
  { keys: 'mod+]',       description: 'Investigation stack: forward',          category: 'investigation' },
  { keys: 'mod+alt+s',   description: 'Go to APM Services',                    category: 'navigation' },
  { keys: 'mod+alt+a',   description: 'Go to Alerts',                          category: 'navigation' },
  { keys: 'mod+alt+d',   description: 'Go to Dashboards',                      category: 'navigation' },
  { keys: 'mod+alt+t',   description: 'Go to Traces (Investigate)',            category: 'navigation' },
  { keys: 'mod+alt+l',   description: 'Go to Logs (Investigate)',              category: 'navigation' },
  { keys: 'mod+alt+r',   description: 'Go to APM User Experience',             category: 'navigation' },
  { keys: 'mod+alt+f',   description: 'Go to Functions',                       category: 'navigation' },
  { keys: 'mod+alt+i',   description: 'Go to IAM',                             category: 'navigation' },
  { keys: 'mod+alt+e',   description: 'Open time window picker',               category: 'time' },
  { keys: 'mod+alt+p',   description: 'Pin / unpin current cursor as anchor',  category: 'time' },
  { keys: 'mod+alt+y',   description: 'Copy investigation link',               category: 'investigation' },
  { keys: 'mod+down',    description: 'Move selection down',                   category: 'navigation' },
  { keys: 'mod+up',      description: 'Move selection up',                     category: 'navigation' },
  { keys: 'mod+enter',   description: 'Activate focused row',                  category: 'navigation' },
];

export function asBinding(doc: BindingDoc, handler: Binding['handler']): Binding {
  return { keys: doc.keys, description: doc.description, category: doc.category, handler };
}
