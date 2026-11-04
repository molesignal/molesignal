import { useTranslation } from 'react-i18next';

/** What the backend calls root wherever an account's roles are shown. */
const SUPER_ADMINISTRATOR = 'Super administrator';

/**
 * A role as the interface shows it: as the organization named it, and root in the reader's language,
 * since no role of any organization stands behind it.
 */
export function useRoleName(): (role: string) => string {
  const { t } = useTranslation('common');

  return (role) => (role === SUPER_ADMINISTRATOR ? t('access.super_administrator') : role);
}
