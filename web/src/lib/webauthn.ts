/**
 * The browser's WebAuthn calls, with binary fields as base64url the way the server
 * speaks them. The server (webauthn-rs) makes the options and checks the answers;
 * this only converts between its JSON and the browser's ArrayBuffers.
 */

type DescriptorJson = Omit<PublicKeyCredentialDescriptor, 'id'> & { id: string };

/** The options the server gives for creating a passkey. */
export interface PasskeyCreationOptions {
  publicKey: Omit<
    PublicKeyCredentialCreationOptions,
    'challenge' | 'user' | 'excludeCredentials'
  > & {
    challenge: string;
    user: Omit<PublicKeyCredentialUserEntity, 'id'> & { id: string };
    excludeCredentials?: DescriptorJson[];
  };
}

/** The options the server gives for signing with a passkey. */
export interface PasskeyRequestOptions {
  publicKey: Omit<PublicKeyCredentialRequestOptions, 'challenge' | 'allowCredentials'> & {
    challenge: string;
    allowCredentials?: DescriptorJson[];
  };
}

/** A passkey the browser created, for the server to check and keep. */
export interface PasskeyRegistration {
  id: string;
  rawId: string;
  type: string;
  response: {
    attestationObject: string;
    clientDataJSON: string;
    transports: string[];
  };
  clientExtensionResults: AuthenticationExtensionsClientOutputs;
}

/** A signature the browser made with a passkey, for the server to check. */
export interface PasskeyAssertion {
  id: string;
  rawId: string;
  type: string;
  response: {
    authenticatorData: string;
    clientDataJSON: string;
    signature: string;
    userHandle: string | null;
  };
  clientExtensionResults: AuthenticationExtensionsClientOutputs;
}

/** One operation handed out by the server: `ceremony` comes back with the answer. */
export interface PasskeyCeremony<Options> {
  ceremony: string;
  options: Options;
}

function fromBase64url(text: string): ArrayBuffer {
  const binary = atob(text.replace(/-/g, '+').replace(/_/g, '/'));
  const bytes = new Uint8Array(binary.length);

  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }

  return bytes.buffer;
}

function toBase64url(buffer: ArrayBuffer): string {
  let binary = '';

  for (const byte of new Uint8Array(buffer)) {
    binary += String.fromCharCode(byte);
  }

  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function descriptors(list: DescriptorJson[] | undefined) {
  return list?.map((descriptor) => ({ ...descriptor, id: fromBase64url(descriptor.id) }));
}

/** This browser can use passkeys at all. */
export function passkeysSupported(): boolean {
  return typeof window !== 'undefined' && typeof window.PublicKeyCredential === 'function';
}

/** This browser can offer passkeys in the autofill of a username field. */
export async function passkeyAutofillSupported(): Promise<boolean> {
  if (!passkeysSupported()) return false;

  try {
    return (await PublicKeyCredential.isConditionalMediationAvailable?.()) ?? false;
  } catch {
    return false;
  }
}

/** Asks the browser to create a passkey. Call it from a click: Safari refuses it otherwise. */
export async function createPasskey(options: PasskeyCreationOptions): Promise<PasskeyRegistration> {
  const { publicKey } = options;
  const { excludeCredentials, ...rest } = publicKey;
  const excluded = descriptors(excludeCredentials);

  const credential = (await navigator.credentials.create({
    publicKey: {
      ...rest,
      challenge: fromBase64url(publicKey.challenge),
      user: { ...publicKey.user, id: fromBase64url(publicKey.user.id) },
      ...(excluded ? { excludeCredentials: excluded } : {}),
    },
  })) as PublicKeyCredential | null;

  if (!credential) throw new DOMException('no passkey was created', 'NotAllowedError');

  const response = credential.response as AuthenticatorAttestationResponse;

  return {
    id: credential.id,
    rawId: toBase64url(credential.rawId),
    type: credential.type,
    response: {
      attestationObject: toBase64url(response.attestationObject),
      clientDataJSON: toBase64url(response.clientDataJSON),
      transports: response.getTransports?.() ?? [],
    },
    clientExtensionResults: credential.getClientExtensionResults(),
  };
}

/**
 * Asks the browser for a signature with a passkey. Call it from a click, unless
 * `autofill` waits for the user to pick a passkey from a username field's suggestions.
 */
export async function getPasskey(
  options: PasskeyRequestOptions,
  { autofill = false, signal }: { autofill?: boolean; signal?: AbortSignal } = {},
): Promise<PasskeyAssertion> {
  const { publicKey } = options;
  const { allowCredentials, ...rest } = publicKey;
  const allowed = descriptors(allowCredentials);

  const credential = (await navigator.credentials.get({
    publicKey: {
      ...rest,
      challenge: fromBase64url(publicKey.challenge),
      ...(allowed ? { allowCredentials: allowed } : {}),
    },
    ...(autofill ? { mediation: 'conditional' as const } : {}),
    ...(signal ? { signal } : {}),
  })) as PublicKeyCredential | null;

  if (!credential) throw new DOMException('no passkey was used', 'NotAllowedError');

  const response = credential.response as AuthenticatorAssertionResponse;

  return {
    id: credential.id,
    rawId: toBase64url(credential.rawId),
    type: credential.type,
    response: {
      authenticatorData: toBase64url(response.authenticatorData),
      clientDataJSON: toBase64url(response.clientDataJSON),
      signature: toBase64url(response.signature),
      userHandle: response.userHandle ? toBase64url(response.userHandle) : null,
    },
    clientExtensionResults: credential.getClientExtensionResults(),
  };
}

/** Why the browser did not finish: the person backed out, or something the page can explain. */
export type PasskeyFailure = 'cancelled' | 'already_added' | 'wrong_address' | 'other';

export function passkeyFailure(error: unknown): PasskeyFailure {
  if (!(error instanceof DOMException)) return 'other';

  switch (error.name) {
    // A closed prompt and a timed-out one look the same to the page.
    case 'NotAllowedError':
    case 'AbortError':
      return 'cancelled';
    case 'InvalidStateError':
      return 'already_added';
    // The page's address does not belong to the server's relying party.
    case 'SecurityError':
      return 'wrong_address';
    default:
      return 'other';
  }
}

/**
 * Tells the passkey's provider that the server no longer knows it, so it stops
 * being offered. Browsers without this call keep offering it.
 */
export function forgetPasskey(rpId: string | undefined, credentialId: string): void {
  const signal = (
    PublicKeyCredential as unknown as {
      signalUnknownCredential?: (credential: {
        rpId: string;
        credentialId: string;
      }) => Promise<void>;
    }
  ).signalUnknownCredential;

  if (!rpId || !signal) return;

  signal.call(PublicKeyCredential, { rpId, credentialId }).catch(() => undefined);
}
