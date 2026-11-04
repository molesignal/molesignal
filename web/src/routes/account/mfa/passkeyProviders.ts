/**
 * Where a passkey is kept, from the authenticator model (AAGUID) the browser
 * reported when it was created. The models are the ones in the community list
 * `passkeydeveloper/passkey-authenticator-aaguids`; an unknown or hidden model has
 * no name, and the page says only "Passkey".
 */

/** Names worded differently in each language: their keys under `security.passkeys.providers`. */
const WORDED: Record<string, string> = {
  fbfc3007154e4ecc8c0b6e020557d7bd: 'icloud_keychain',
  dd4ec289e01d41c9bb8970fa845d4bf2: 'icloud_keychain',
  ea9b8d664d011d213ce4b6b48cb575d4: 'google_password_manager',
  adce000235bcc60a648b0b25f1f05503: 'chrome_on_mac',
  d345266801fd4c12926c83a4204853aa: 'microsoft_password_manager',
};

/** Product names, the same in every language. */
const BRANDS: Record<string, string> = {
  '08987058cadc4b81b6e130de50dcbe96': 'Windows Hello',
  '9ddd1817af5a4672a2b93e3dd95000a9': 'Windows Hello',
  '6028b017b1d44c02b4b3afcdafc96bb2': 'Windows Hello',
  bada5566a7aa401fbd9645619a55120d: '1Password',
  d548826e79b4db40a3d811116f7e8349: 'Bitwarden',
  '531126d6e717415c93203d9aa6981239': 'Dashlane',
  b84e404815dc4dd08640f4f60813c8af: 'NordPass',
  '0ea242b443c44a1b8b17dd6d0b6baec6': 'Keeper',
  f38095407f1449c1a8b38f813b225541: 'Enpass',
  '53414d53554e47000000000000000000': 'Samsung Pass',
  '50726f746f6e5061737350726f746f6e': 'Proton Pass',
};

/** The name of where a passkey is kept; `translate` looks up the worded ones. */
export function passkeyProvider(
  aaguid: string | null,
  translate: (key: string) => string,
): string | null {
  if (!aaguid) return null;

  const worded = WORDED[aaguid];

  return worded ? translate(`security.passkeys.providers.${worded}`) : (BRANDS[aaguid] ?? null);
}
