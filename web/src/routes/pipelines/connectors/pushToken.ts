/**
 * A token for a push connector: 32 random bytes, written in characters that
 * are safe in an address and in a header, since both are how it travels.
 */
export function newPushToken(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(32));

  return btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
}
