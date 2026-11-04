import { Navigate, useLocation } from 'react-router-dom';

import { isProductPathDenied, useProductAccess } from './access';

/** The pages of a module to offer: a tab that could only lead to "no permission" is not one. */
export function useOpenablePages<Page>(
  pages: readonly Page[],
  pathOf: (page: Page) => string,
): readonly Page[] {
  const access = useProductAccess();

  return pages.filter((page) => !isProductPathDenied(pathOf(page), access));
}

/**
 * Where a module opens: its first page the reader may open, so that holding the permission of one
 * page is enough to get in. Keyed by the navigation: a click on the sidebar that lands here while
 * this redirect is under way would otherwise cancel it and leave the page empty.
 */
export function FirstOpenablePage({ paths }: { paths: readonly [string, ...string[]] }) {
  const access = useProductAccess();
  const { key } = useLocation();
  const landing = paths.find((path) => !isProductPathDenied(path, access)) ?? paths[0];

  return <Navigate key={key} to={landing} replace />;
}
