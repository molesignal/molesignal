import { hasPermission, type ProductAccess } from '@/product/access';

/**
 * Whether the reader may see scheduled inspections and their reports. Mole
 * Agent does the inspecting and the reports quote logs, so the backend asks
 * for both permissions; so does the page.
 */
export function canReadInspections(access: ProductAccess | null): boolean {
  return hasPermission('agent.use', access) && hasPermission('streams.query', access);
}

/**
 * Whether the reader may save, switch, run and delete inspections. An
 * inspection runs with the access of whoever saved it, which is why saving
 * takes what reading takes as well.
 */
export function canManageInspections(access: ProductAccess | null): boolean {
  return hasPermission('inspections.manage', access) && canReadInspections(access);
}
