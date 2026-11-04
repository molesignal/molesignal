/**
 * The model of a copy: a new dashboard of the person who copies it, who can change it, even where
 * the one it was made from is read-only, and which is not the built-in one it was copied from.
 */
export function copiedModel(model: Record<string, unknown>, title: string): Record<string, unknown> {
  const extensions = { ...((model.extensions ?? {}) as Record<string, unknown>) };

  delete extensions.builtin;

  return { ...model, uid: '', title, editable: true, extensions };
}
