import { describe, expect, it } from 'vitest';

import { auditReaderName } from './auditReader';

const base = { id: 'p1', name: 'copy' };

describe('auditReaderName', () => {
  it('prefers the display name, then the email, then the id of a user who is gone', () => {
    const audit_reader = { id: 'u1', displayName: 'Ada', email: 'ada@example.com' };

    expect(auditReaderName({ ...base, auditReaderId: 'u1', auditReader: audit_reader })).toBe('Ada');
    expect(auditReaderName({ ...base, auditReaderId: 'u1', auditReader: { ...audit_reader, displayName: ' ' } })).toBe(
      'ada@example.com',
    );
    expect(auditReaderName({ ...base, auditReaderId: 'u1', auditReader: null })).toBe('u1');
  });

  it('names nobody for a pipeline that reads as nobody', () => {
    expect(auditReaderName(base)).toBeNull();
    expect(auditReaderName({ ...base, auditReaderId: null, auditReader: null })).toBeNull();
    expect(auditReaderName(null)).toBeNull();
  });
});
