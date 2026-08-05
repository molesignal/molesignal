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
  removeRelationship,
  removeRoleBinding,
  revokeCrossOrgGrant,
} from './iam';
export type {
  CrossOrgGrant,
  CrossOrgGrantStatus,
  CreateRoleBindingPayload,
  MutationResponse,
  IamShareTarget,
  PrincipalType,
  ResourceRelationship,
  RoleBinding,
} from './iam';
