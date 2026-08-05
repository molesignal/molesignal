export const DEFAULT_HOME_STREAM_ROWS = 8;

interface HomeStreamRowCountInput {
  viewportHeight: number;
  headerHeight: number;
  rowHeight: number;
  totalRows: number;
  fallbackRows?: number;
}

export function calculateHomeStreamRowCount({
  viewportHeight,
  headerHeight,
  rowHeight,
  totalRows,
  fallbackRows = DEFAULT_HOME_STREAM_ROWS,
}: HomeStreamRowCountInput): number {
  const availableRows = Math.max(0, Math.floor(totalRows));
  if (availableRows === 0) return 0;

  const fallback = Math.min(
    availableRows,
    Math.max(1, Math.floor(fallbackRows)),
  );
  if (
    !Number.isFinite(viewportHeight) ||
    !Number.isFinite(headerHeight) ||
    !Number.isFinite(rowHeight) ||
    viewportHeight <= 0 ||
    rowHeight <= 0
  ) {
    return fallback;
  }

  const contentHeight = Math.max(0, viewportHeight - Math.max(0, headerHeight));
  const fittedRows = Math.floor(contentHeight / rowHeight);
  return Math.min(availableRows, Math.max(1, fittedRows));
}

interface HomeStreamViewportFillInput {
  viewportHeight: number;
  headerHeight: number;
  rowHeight: number;
  visibleRows: number;
}

export function shouldFillHomeStreamViewport({
  viewportHeight,
  headerHeight,
  rowHeight,
  visibleRows,
}: HomeStreamViewportFillInput): boolean {
  if (
    !Number.isFinite(viewportHeight) ||
    !Number.isFinite(headerHeight) ||
    !Number.isFinite(rowHeight) ||
    !Number.isFinite(visibleRows) ||
    viewportHeight <= 0 ||
    rowHeight <= 0 ||
    visibleRows <= 0
  ) {
    return false;
  }

  const remainingHeight =
    viewportHeight -
    Math.max(0, headerHeight) -
    rowHeight * Math.floor(visibleRows);
  return remainingHeight >= 0 && remainingHeight < rowHeight;
}
