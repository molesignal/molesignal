import { useTranslation } from 'react-i18next';
import { Link, useLocation } from 'react-router-dom';

import { canAccessProductRoute, useProductAccess } from '@/product/access';
import { PRODUCT_ROUTES, type ProductRouteMeta } from '@/product/ia';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { SurfaceStatePage } from '@/shell/SurfaceWorkbench';

import { suggestRoutes } from './gate/suggestRoutes';

/**
 * 认证区内未匹配到的路由（含错拼的 settings/iam 子路径）落到这里，渲染保留外壳的
 * 友好 404，而不是 React Router 默认的整页英文 ErrorBoundary（侧栏/顶栏全失）。
 * 地址里的词能对上某个页面时，一并给出可能想去的页面。
 */
export function NotFound() {
  const { t } = useTranslation('shell');
  const { t: tn } = useTranslation('nav');
  const location = useLocation();
  const access = useProductAccess();

  // Only pages this person can open, and only addresses that can be followed as they stand.
  const candidates = (PRODUCT_ROUTES as readonly ProductRouteMeta[]).filter(
    (route) => !route.path.includes(':') && !route.path.includes('*') && canAccessProductRoute(route, access),
  );
  const suggestions = suggestRoutes(location.pathname, candidates);

  return (
    <SurfaceStatePage>
      <ProductState
        variant="empty"
        title={t('not_found.title')}
        description={t('not_found.description')}
        action={
          <div className="flex flex-col items-center gap-3">
            <Link to="/home">
              <ChromeButton variant="primary">{t('not_found.back_home')}</ChromeButton>
            </Link>
            {suggestions.length > 0 && (
              <nav aria-label={t('not_found.suggestions')} className="flex flex-col items-center gap-1 pt-1" data-not-found-suggestions>
                <span className="font-sans text-xs text-tx-3">{t('not_found.suggestions')}</span>
                {suggestions.map((route) => (
                  <Link
                    key={route.id}
                    to={route.path}
                    className="font-sans text-sm text-[var(--indigo-ink)] hover:underline focus-visible:underline focus-visible:outline-none"
                  >
                    {tn(route.labelKey)}
                  </Link>
                ))}
              </nav>
            )}
          </div>
        }
      />
    </SurfaceStatePage>
  );
}
