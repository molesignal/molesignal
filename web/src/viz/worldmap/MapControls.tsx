import { Maximize2, Minus, Plus } from 'lucide-react';
import type * as React from 'react';

import { IconButton } from '@/shell/chrome';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shell/ui/tooltip';

export interface MapControlLabels {
  zoomIn: string;
  zoomOut: string;
  /** Shows the whole map again. */
  reset: string;
}

/** Zoom in, zoom out, and the whole map again: the buttons of a map, in a column at its corner. */
export function MapControls({
  labels,
  canZoomIn,
  canZoomOut,
  onZoomIn,
  onZoomOut,
  onReset,
}: {
  labels: MapControlLabels;
  canZoomIn: boolean;
  canZoomOut: boolean;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onReset: () => void;
}) {
  return (
    <TooltipProvider delayDuration={250}>
      <div data-map-controls className="overflow-hidden rounded-md border border-bd-0 bg-bg-1/95">
        <MapControl label={labels.zoomIn} disabled={!canZoomIn} onClick={onZoomIn}>
          <Plus aria-hidden className="h-4 w-4" />
        </MapControl>
        <div className="h-px bg-bd-0" />
        <MapControl label={labels.zoomOut} disabled={!canZoomOut} onClick={onZoomOut}>
          <Minus aria-hidden className="h-4 w-4" />
        </MapControl>
        <div className="h-px bg-bd-0" />
        <MapControl label={labels.reset} disabled={!canZoomOut} onClick={onReset}>
          <Maximize2 aria-hidden className="h-3.5 w-3.5" />
        </MapControl>
      </div>
    </TooltipProvider>
  );
}

function MapControl({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <IconButton aria-label={label} disabled={disabled} onClick={onClick} className="h-11 w-11 rounded-none sm:h-9 sm:w-9">
          {children}
        </IconButton>
      </TooltipTrigger>
      <TooltipContent side="left">{label}</TooltipContent>
    </Tooltip>
  );
}
