/**
 * Semantic access-grant facade.
 *
 * Semantic facade over the IAM access API.
 */
export {
  acceptCrossOrgGrant,
  createCrossOrgGrant,
  createRelationship,
  createRoleBinding,
  listCrossOrgGrants,
  listRelationships,
  listRoleBindings,
  listShareTargets,
  platformAccountsByIds,
  removeRelationship,
  removeRoleBinding,
  revokeCrossOrgGrant,
  searchPlatformAccounts,
} from './iam';
export type {
  CrossOrgGrant,
  CrossOrgGrantStatus,
  CreateRoleBindingPayload,
  MutationResponse,
  IamShareTarget,
  PlatformAccount,
  PrincipalType,
  ResourceRelationship,
  RoleBinding,
} from './iam';
