import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useLocation } from 'react-router-dom';

import { writeClipboardText } from '@/lib/clipboard';
import { findProductRoute } from '@/product/ia';
import { useRoleName } from '@/product/roleName';
import { ProductState } from '@/product/states';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { SurfaceStatePage } from '@/shell/SurfaceWorkbench';
import { useAuthStore } from '@/stores/auth';

/**
 * A page this person's role does not include. It is said where they asked for
 * it, with who can change that and something to send them: sent elsewhere without
 * a word, nobody can tell a missing permission from a broken link.
 */
export function AccessDenied({ fallback }: { fallback: string }) {
  const { t } = useTranslation('shell');
  const { t: tn } = useTranslation('nav');
  const location = useLocation();
  const roleName = useRoleName();
  const role = roleName(useAuthStore((state) => state.ctx?.displayRole ?? ''));
  const [copied, setCopied] = React.useState(false);

  const route = findProductRoute(location.pathname);
  const page = route ? tn(route.labelKey) : location.pathname;

  const copyRequest = async () => {
    try {
      await writeClipboardText(t('access_denied.request_text', { page, path: location.pathname, role }));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // The clipboard is blocked: the page's name is on screen to pass on by hand.
    }
  };

  return (
    <SurfaceStatePage>
      <ProductState
        variant="permission-denied"
        data-testid="access-denied"
        title={t('access_denied.title')}
        description={t('access_denied.description', { role, page })}
        action={
          <>
            <Link
              to={fallback}
              replace
              className="ms-press inline-flex h-9 items-center rounded-md bg-indigo px-4 font-sans text-sm font-strong text-white hover:brightness-90 focus-visible:brightness-90 focus-visible:outline-none"
            >
              {t('access_denied.go_on')}
            </Link>
            <CopyIconButton
              onClick={() => void copyRequest()}
              label={t('access_denied.copy_request')}
              copied={copied}
              copiedLabel={t('access_denied.copied')}
            />
          </>
        }
      />
    </SurfaceStatePage>
  );
}
