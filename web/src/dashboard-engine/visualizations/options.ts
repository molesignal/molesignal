export function resolveVisualizationOptions(
  defaults: Readonly<Record<string, unknown>>,
  configured: Readonly<Record<string, unknown>>,
): Record<string, unknown> {
  return { ...defaults, ...configured };
}

export function transitionVisualizationOptions(
  targetDefaults: Readonly<Record<string, unknown>>,
  current: Readonly<Record<string, unknown>>,
): Record<string, unknown> {
  const next = { ...targetDefaults };
  for (const key of Object.keys(targetDefaults)) {
    if (Object.hasOwn(current, key)) next[key] = current[key];
  }
  return next;
}
