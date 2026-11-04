import type { LdapPreset } from './types';

export const LDAP_PRESETS: LdapPreset[] = [
  {
    platform: 'active_directory',
    kind: 'ldap',
    // Sign in with the Windows account name, the UPN or the mailbox.
    userFilter:
      '(&(objectCategory=person)(objectClass=user)(|(sAMAccountName={username})(userPrincipalName={username})(mail={username})))',
    fieldMapping: {
      subject: 'dn',
      email: 'mail',
      displayName: 'displayName',
      groups: 'memberOf',
    },
    groups: true,
    hint: 'active_directory',
    placeholders: {
      url: 'ldaps://dc01.corp.example.com:636',
      baseDn: 'DC=corp,DC=example,DC=com',
      bindDn: 'CN=svc-molesignal,OU=Service Accounts,DC=corp,DC=example,DC=com',
    },
  },
  {
    platform: 'openldap',
    kind: 'ldap',
    userFilter: '(&(objectClass=inetOrgPerson)(|(uid={username})(mail={username})))',
    fieldMapping: {
      subject: 'dn',
      email: 'mail',
      displayName: 'cn',
      groups: 'memberOf',
    },
    groups: true,
    hint: 'openldap',
    placeholders: {
      url: 'ldaps://ldap.example.com:636',
      baseDn: 'ou=people,dc=example,dc=com',
      bindDn: 'cn=reader,dc=example,dc=com',
    },
  },
  {
    platform: 'freeipa',
    kind: 'ldap',
    userFilter: '(&(objectClass=person)(uid={username}))',
    fieldMapping: {
      subject: 'dn',
      email: 'mail',
      displayName: 'displayName',
      groups: 'memberOf',
    },
    groups: true,
    hint: 'freeipa',
    placeholders: {
      url: 'ldaps://ipa.example.com:636',
      baseDn: 'cn=users,cn=accounts,dc=example,dc=com',
      bindDn: 'uid=molesignal,cn=sysaccounts,cn=etc,dc=example,dc=com',
    },
  },
];
