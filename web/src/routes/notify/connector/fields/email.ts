import { TIMEOUT, address, type FieldSpec } from './spec';

/** How an SMTP connector signs in: a user name and password, or OAuth 2.0 as a Microsoft 365 or
 * Google Workspace mailbox. */
export const SMTP_AUTH_MODES = ['password', 'microsoft', 'google'];

/** The ways of signing in that send as a mailbox: the user name is its address. */
export const MAILBOX_AUTH_MODES = ['microsoft', 'google'];

const MICROSOFT = { key: 'auth', values: ['microsoft'] };
const GOOGLE = { key: 'auth', values: ['google'] };

/** The settings of email. With OAuth the mailbox that sends comes beside the sender address; the
 * Microsoft sign-in address, worldwide Microsoft 365's, is shown but not edited. */
export const EMAIL_FIELDS: Record<string, FieldSpec[]> = {
  email_smtp: [
    { key: 'host' },
    { key: 'port', type: 'number', defaultValue: '587' },
    { key: 'auth', type: 'select', options: SMTP_AUTH_MODES, defaultValue: 'password', wide: true },
    { key: 'tenantId', when: MICROSOFT },
    { key: 'clientId', when: MICROSOFT },
    { key: 'clientSecret', type: 'password', when: MICROSOFT },
    address('loginBaseUrl', MICROSOFT),
    { key: 'credentials', type: 'json', when: GOOGLE },
    { key: 'username', optional: true },
    { key: 'password', type: 'password', when: { key: 'auth', values: ['password'] } },
    { key: 'from' },
    { key: 'tls', type: 'select', options: ['none', 'starttls', 'tls'], defaultValue: 'starttls' },
    TIMEOUT,
  ],
};
