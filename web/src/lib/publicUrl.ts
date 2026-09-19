export function resolvePublicBaseUrl(
  configuredExternalUrl: string | null | undefined,
  browserOrigin: string,
): string {
  return (configuredExternalUrl?.trim() || browserOrigin.trim()).replace(
    /\/+$/,
    '',
  );
}

export function resolvePublicUrl(value: string, publicBaseUrl: string): string {
  const candidate = value.trim();
  if (!candidate) return candidate;

  try {
    const base = `${publicBaseUrl.replace(/\/+$/, '')}/`;
    const resolved = new URL(candidate, base);
    if (resolved.protocol !== 'http:' && resolved.protocol !== 'https:') {
      return candidate;
    }
    return resolved.toString();
  } catch {
    return candidate;
  }
}
