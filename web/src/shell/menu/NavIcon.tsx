import { LayoutDashboard, type LucideIcon, SquareFunction, TableProperties } from 'lucide-react';

import { cn } from '@/shell/lib/cn';

/** The stroke of the menu's 16px icons, on lucide's 24-unit grid. */
const NAV_STROKE = 1.75;

/** The stroke that draws an icon of `size` px as thick as the menu's 16px icons. */
function navStroke(size: number): number {
  return (NAV_STROKE * 16) / size;
}

/**
 * Glyphs that fill a square of their box: a framed function, a table, a dashboard's panes. Beside a
 * bell, a shield or a gear, which are round, pointed or open, they read a size larger.
 */
const FILLS_ITS_BOX: ReadonlySet<LucideIcon> = new Set([LayoutDashboard, SquareFunction, TableProperties]);

/**
 * A destination's icon in the main menu. Every icon has a 16px box, so the labels of a column start
 * on one line; one that fills its box is drawn at 15px inside it, as thick as the others, so the
 * menu reads as one size.
 */
export function NavIcon({ icon: Icon, className }: { icon: LucideIcon; className?: string }) {
  const fills = FILLS_ITS_BOX.has(Icon);

  return (
    <Icon
      strokeWidth={fills ? navStroke(15) : NAV_STROKE}
      className={cn('h-4 w-4 shrink-0', fills && 'scale-[0.9375]', className)}
    />
  );
}
