import { randomBytes } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { Express } from 'express';

/**
 * Multi-factor authentication of the mock user. The authenticator code is
 * {@link MOCK_TOTP_CODE} whatever the time; every other code is wrong.
 */
export const MOCK_TOTP_CODE = '123456';

/** The recovery codes a first setup hands out: ten characters each, like the server's. */
export const MOCK_RECOVERY_CODES = Array.from(
  { length: 10 },
  (_, index) => `rcvr${index}-codes`,
);

/** The address the mock user's authenticator setup scans. */
const MOCK_OTPAUTH_URL =
  'otpauth://totp/MoleSignal:dev%40molesignal.local?secret=JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP&issuer=MoleSignal&algorithm=SHA1&digits=6&period=30';

/**
 * The QR code of {@link MOCK_OTPAUTH_URL}, drawn by the `qrcode` crate with the
 * server's own settings, so the setup dialog looks as it does for real.
 */
const MOCK_QR_SVG = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), 'data', 'totp-qr.svg'),
  'utf8',
);

const WRONG_CODE = {
  error: 'invalid_argument',
  message: 'invalid argument: the verification code is not correct',
};

const WRONG_PASSWORD = {
  error: 'invalid_argument',
  message: 'invalid argument: the password is not correct',
};

function normalizedRecovery(code: string): string {
  return code.replace(/[\s-]/g, '').toLowerCase();
}

/** The mock user's password, the only one that confirms adding a second factor. */
export const MOCK_PASSWORD = 'correct horse battery';

/** The AAGUID the mock reports for a new passkey, so the list has a place to name. */
const MOCK_ICLOUD_KEYCHAIN = 'fbfc3007154e4ecc8c0b6e020557d7bd';

/** The host the specs open the pages on; a passkey is bound to it. */
const MOCK_RP_ID = 'localhost';

/** Base64url of random bytes, the way the server encodes challenges. */
function randomBase64url(): string {
  return randomBytes(32).toString('base64url');
}

interface MockPasskey {
  id: string;
  credentialId: string;
  name: string;
  aaguid: string | null;
  createdAt: number;
  lastUsedAt: number | null;
}

/**
 * `/auth/signin` and its second step, the account's own settings under
 * `/me/mfa` and `/me/passkeys`, signing out other devices and an administrator's
 * reset. Password sign-in asks for the second step while the mock user has a
 * second factor. Passkeys take whatever a browser's virtual authenticator answers:
 * the mock checks only that the credential is one it knows.
 */
export function registerMfaRoutes(app: Express, login: object): void {
  const state = {
    on: false,
    since: null as number | null,
    recovery: [] as string[],
    passkeys: [] as MockPasskey[],
    policy: { requireMfa: false, ssoOnly: false },
  };

  const ssoOnly = {
    error: 'forbidden',
    message: 'forbidden: acme-prod only allows single sign-on: sign in through its identity provider',
  };

  const mfaOn = () => state.on || state.passkeys.length > 0;

  const passkeyHolds = (proof: { credential?: { id?: string } }): boolean => {
    const known = state.passkeys.find((passkey) => passkey.credentialId === proof.credential?.id);
    if (known) known.lastUsedAt = Date.now() * 1000;
    return known !== undefined;
  };

  const proofHolds = (method: unknown, code: unknown, proof: object = {}): boolean => {
    if (method === 'passkey') return passkeyHolds(proof);
    if (typeof code !== 'string') return false;
    if (method === 'totp') return code === MOCK_TOTP_CODE;
    if (method !== 'recovery_code') return false;
    const index = state.recovery.indexOf(normalizedRecovery(code));
    if (index < 0) return false;
    state.recovery.splice(index, 1);
    return true;
  };

  /** Adding a second factor: an existing one while there is one, the password before that. */
  const addingHolds = (proof: { method?: string; code?: string; password?: string } = {}) =>
    mfaOn() ? proofHolds(proof.method, proof.code, proof) : proof.password === MOCK_PASSWORD;

  const wrongProof = (proof: { method?: string } = {}) =>
    proof.method === 'password' || !mfaOn() ? WRONG_PASSWORD : WRONG_CODE;

  const creationOptions = () => ({
    ceremony: `mock-ceremony-${randomBase64url()}`,
    options: {
      publicKey: {
        rp: { name: 'MoleSignal', id: MOCK_RP_ID },
        user: {
          id: randomBase64url(),
          name: 'dev@molesignal.local',
          displayName: 'Dev',
        },
        challenge: randomBase64url(),
        pubKeyCredParams: [{ type: 'public-key', alg: -7 }],
        timeout: 300_000,
        excludeCredentials: state.passkeys.map((passkey) => ({
          type: 'public-key',
          id: passkey.credentialId,
        })),
        authenticatorSelection: {
          residentKey: 'required',
          requireResidentKey: true,
          userVerification: 'required',
        },
        attestation: 'none',
        extensions: { credProps: true },
      },
    },
  });

  const totpSetup = () => ({
    setup: 'mock-setup',
    secret: 'JBSW Y3DP EHPK 3PXP JBSW Y3DP EHPK 3PXP',
    otpauthUrl: MOCK_OTPAUTH_URL,
    qrSvg: MOCK_QR_SVG,
    expiresAt: Date.now() * 1000 + 600_000_000,
  });

  const addPasskey = (body: { credential?: { id?: string }; name?: string } = {}) => {
    const first = !mfaOn();
    const passkey: MockPasskey = {
      id: `passkey-${state.passkeys.length + 1}`,
      credentialId: body.credential?.id ?? '',
      name: body.name ?? '',
      aaguid: MOCK_ICLOUD_KEYCHAIN,
      createdAt: Date.UTC(2026, 9, 30, 9) * 1000,
      lastUsedAt: null,
    };
    state.passkeys.push(passkey);
    if (first) state.recovery = MOCK_RECOVERY_CODES.map(normalizedRecovery);
    const { credentialId: _kept, ...view } = passkey;
    return { passkey: view, recoveryCodes: first ? MOCK_RECOVERY_CODES : null };
  };

  const turnOnTotp = () => {
    const first = !mfaOn();
    state.on = true;
    state.since = Date.UTC(2026, 9, 30, 8) * 1000;
    if (first) state.recovery = MOCK_RECOVERY_CODES.map(normalizedRecovery);
    return first ? MOCK_RECOVERY_CODES : null;
  };

  const requestOptions = (allow: MockPasskey[]) => ({
    ceremony: `mock-ceremony-${randomBase64url()}`,
    options: {
      publicKey: {
        challenge: randomBase64url(),
        timeout: 300_000,
        rpId: MOCK_RP_ID,
        allowCredentials: allow.map((passkey) => ({
          type: 'public-key',
          id: passkey.credentialId,
        })),
        userVerification: 'required',
      },
    },
  });

  app.post('/api/v1/auth/signin', (_req, res) => {
    if (state.policy.ssoOnly) {
      res.status(403).json(ssoOnly);
      return;
    }
    if (!mfaOn() && state.policy.requireMfa) {
      res.json({
        enroll: {
          challenge: 'mock-enroll',
          methods: ['passkey', 'totp'],
          organizationName: 'acme-prod',
          expiresAt: Date.now() * 1000 + 600_000_000,
        },
      });
      return;
    }
    if (!mfaOn()) {
      res.json({ login });
      return;
    }
    res.json({
      mfa: {
        challenge: 'mock-challenge',
        methods: [
          ...(state.on ? ['totp'] : []),
          ...(state.passkeys.length > 0 ? ['passkey'] : []),
          'recovery_code',
        ],
        expiresAt: Date.now() * 1000 + 300_000_000,
      },
    });
  });

  app.post('/api/v1/auth/signin/verify/passkey', (req, res) => {
    if (req.body?.challenge !== 'mock-challenge') {
      res.status(401).json({
        error: 'unauthorized',
        message: 'unauthorized: this sign-in has expired; sign in again',
      });
      return;
    }
    res.json(requestOptions(state.passkeys));
  });

  app.post('/api/v1/auth/signin/passkey/options', (_req, res) => res.json(requestOptions([])));

  app.post('/api/v1/auth/signin/enroll/totp', (_req, res) => res.json(totpSetup()));

  app.post('/api/v1/auth/signin/enroll/totp/confirm', (req, res) => {
    if (req.body?.setup !== 'mock-setup' || req.body?.code !== MOCK_TOTP_CODE) {
      res.status(400).json(WRONG_CODE);
      return;
    }
    res.json({ login, recoveryCodes: turnOnTotp() });
  });

  app.post('/api/v1/auth/signin/enroll/passkey/options', (_req, res) =>
    res.json(creationOptions()),
  );

  app.post('/api/v1/auth/signin/enroll/passkey', (req, res) => {
    const { recoveryCodes } = addPasskey(req.body);
    res.json({ login, recoveryCodes: recoveryCodes });
  });

  app.get('/api/v1/settings/signin-policy', (_req, res) => res.json(state.policy));

  app.put('/api/v1/settings/signin-policy', (req, res) => {
    state.policy = { requireMfa: req.body.requireMfa, ssoOnly: req.body.ssoOnly };
    res.json(state.policy);
  });

  app.post('/api/v1/auth/signin/passkey', (req, res) => {
    if (state.policy.ssoOnly) {
      res.status(403).json(ssoOnly);
      return;
    }
    if (!passkeyHolds(req.body ?? {})) {
      res.status(401).json({
        error: 'unauthorized',
        message: 'unauthorized: this passkey is not registered here',
      });
      return;
    }
    res.json({ login });
  });

  app.post('/api/v1/auth/signin/verify', (req, res) => {
    const { challenge, method, code, ...proof } = req.body ?? {};
    if (challenge !== 'mock-challenge') {
      res.status(401).json({
        error: 'unauthorized',
        message: 'unauthorized: this sign-in has expired; sign in again',
      });
      return;
    }
    const before = state.recovery.length;
    if (!proofHolds(method, code, proof)) {
      res.status(401).json({
        error: 'unauthorized',
        message:
          method === 'passkey'
            ? 'unauthorized: the passkey could not be verified'
            : 'unauthorized: the verification code is not correct',
      });
      return;
    }
    const usedRecovery = state.recovery.length < before;
    res.json({ login, recoveryCodesLeft: usedRecovery ? state.recovery.length : null });
  });

  app.get('/api/v1/me/mfa', (_req, res) =>
    res.json({
      totpEnabled: state.on,
      totpEnabledAt: state.since,
      recoveryCodesLeft: mfaOn() ? state.recovery.length : 0,
      passkeys: state.passkeys.map(({ credentialId: _kept, ...passkey }) => passkey),
      passkeysUnavailable: null,
      addingRequires: mfaOn() ? 'factor' : 'password',
      requiredBy: state.policy.requireMfa ? ['acme-prod'] : [],
    }),
  );

  app.post('/api/v1/me/mfa/passkey-proof', (_req, res) =>
    res.json(requestOptions(state.passkeys)),
  );

  app.post('/api/v1/me/passkeys/options', (req, res) => {
    if (!addingHolds(req.body?.proof)) {
      res.status(400).json(wrongProof(req.body?.proof));
      return;
    }
    res.json(creationOptions());
  });

  app.post('/api/v1/me/passkeys', (req, res) => res.json(addPasskey(req.body)));

  app.patch('/api/v1/me/passkeys/:id', (req, res) => {
    const passkey = state.passkeys.find((candidate) => candidate.id === req.params.id);
    if (!passkey) {
      res.status(404).json({ error: 'not_found', message: 'not found: passkey' });
      return;
    }
    passkey.name = req.body?.name ?? '';
    res.status(204).end();
  });

  app.post('/api/v1/me/passkeys/:id/remove', (req, res) => {
    const { method, code, ...proof } = req.body ?? {};
    if (!proofHolds(method, code, proof)) {
      res.status(400).json(WRONG_CODE);
      return;
    }
    state.passkeys = state.passkeys.filter((candidate) => candidate.id !== req.params.id);
    if (!mfaOn()) state.recovery = [];
    res.status(204).end();
  });

  app.post('/api/v1/me/mfa/totp/setup', (req, res) => {
    if (!addingHolds(req.body?.proof)) {
      res.status(400).json(wrongProof(req.body?.proof));
      return;
    }
    res.json(totpSetup());
  });

  app.post('/api/v1/me/mfa/totp/confirm', (req, res) => {
    if (req.body?.setup !== 'mock-setup' || req.body?.code !== MOCK_TOTP_CODE) {
      res.status(400).json(WRONG_CODE);
      return;
    }
    res.json({ recoveryCodes: turnOnTotp() });
  });

  app.post('/api/v1/me/mfa/totp/disable', (req, res) => {
    const { method, code, ...proof } = req.body ?? {};
    if (!state.on || !proofHolds(method, code, proof)) {
      res.status(400).json(WRONG_CODE);
      return;
    }
    state.on = false;
    state.since = null;
    if (!mfaOn()) state.recovery = [];
    res.status(204).end();
  });

  app.post('/api/v1/me/mfa/recovery-codes', (req, res) => {
    const { method, code, ...proof } = req.body ?? {};
    if (!mfaOn() || !proofHolds(method, code, proof)) {
      res.status(400).json(WRONG_CODE);
      return;
    }
    const codes = Array.from({ length: 10 }, (_, index) => `newc${index}-codes`);
    state.recovery = codes.map(normalizedRecovery);
    res.json({ recoveryCodes: codes });
  });

  app.post('/api/v1/me/sessions/revoke-others', (_req, res) => res.status(204).end());
  app.post('/api/v1/users/:id/mfa/reset', (_req, res) => res.status(204).end());
}
