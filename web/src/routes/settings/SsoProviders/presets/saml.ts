import type { SamlPreset } from './types';

/** Claim types Microsoft's identity platform and AD FS put in SAML assertions. */
const CLAIMS = {
  email: 'http://schemas.xmlsoap.org/ws/2005/05/identity/claims/emailaddress',
  name: 'http://schemas.xmlsoap.org/ws/2005/05/identity/claims/name',
  displayName: 'http://schemas.microsoft.com/identity/claims/displayname',
  groups: 'http://schemas.microsoft.com/ws/2008/06/identity/claims/groups',
  adfsGroup: 'http://schemas.xmlsoap.org/claims/Group',
};

export const SAML_PRESETS: SamlPreset[] = [
  {
    platform: 'microsoft',
    kind: 'saml',
    fieldMapping: {
      subject: 'NameID',
      email: CLAIMS.email,
      displayName: CLAIMS.displayName,
      groups: CLAIMS.groups,
    },
    groups: true,
    hint: 'microsoft_saml',
    metadataPlaceholder: 'microsoft',
  },
  {
    platform: 'okta',
    kind: 'saml',
    fieldMapping: {
      subject: 'NameID',
      email: 'email',
      displayName: 'displayName',
      groups: 'groups',
    },
    groups: true,
    hint: 'okta_saml',
    metadataPlaceholder: 'okta',
  },
  {
    platform: 'google',
    kind: 'saml',
    fieldMapping: {
      subject: 'NameID',
      email: 'email',
      displayName: 'name',
      groups: 'groups',
    },
    groups: true,
    hint: 'google_saml',
    metadataPlaceholder: 'xml',
  },
  {
    platform: 'adfs',
    kind: 'saml',
    fieldMapping: {
      subject: 'NameID',
      email: CLAIMS.email,
      displayName: CLAIMS.name,
      groups: CLAIMS.adfsGroup,
    },
    groups: true,
    hint: 'adfs',
    metadataPlaceholder: 'adfs',
  },
  {
    platform: 'jumpcloud',
    kind: 'saml',
    fieldMapping: {
      subject: 'NameID',
      email: 'email',
      displayName: 'name',
      groups: 'memberOf',
    },
    groups: true,
    hint: 'jumpcloud',
    metadataPlaceholder: 'xml',
  },
];
