import * as React from 'react';

/**
 * What the APM navigation card carries besides its tabs.
 *
 * The card is where a reader answers "where am I and what am I looking at", so
 * the answer is in one place: the tabs, the time range at the end of their row,
 * and the entities the page is scoped to in a row under them. The page frame
 * knows the filters; the navigation (the module's, or one service's) draws
 * them, so the two meet through this context.
 */
export interface ApmNavigationSlotsValue {
  /** At the end of the tabs' row: resolution and time range. */
  end?: React.ReactNode;
  /** A row under the tabs: namespace, service, environment, version. */
  scope?: React.ReactNode;
}

export const ApmNavigationSlots = React.createContext<ApmNavigationSlotsValue>({});

/**
 * Which part of the filters a placement draws. The frame renders the same
 * filters twice, once per slot, and each copy draws only its part; on their
 * own (`all`) they are one row.
 */
export type ApmFilterPart = 'all' | 'scope' | 'time';

export const ApmFilterPartContext = React.createContext<ApmFilterPart>('all');
