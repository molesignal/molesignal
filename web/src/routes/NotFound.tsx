import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { GatePage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';

/**
 * 认证区内未匹配到的路由（含错拼的 settings/iam 子路径）落到这里，渲染保留外壳的
 * 友好 404，而不是 React Router 默认的整页英文 ErrorBoundary（侧栏/顶栏全失）。
 */
export function NotFound() {
  const { t } = useTranslation('shell');
  return (
    <GatePage
      title={t('not_found.title')}
      breadcrumbs={null}
      backTo={null}
      state={{
        variant: 'empty',
        title: t('not_found.title'),
        description: t('not_found.description'),
        action: (
          <Link to="/home">
            <ChromeButton variant="primary">{t('not_found.back_home')}</ChromeButton>
          </Link>
        ),
      }}
    />
  );
}
