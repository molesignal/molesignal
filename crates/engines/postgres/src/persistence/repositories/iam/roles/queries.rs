// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IAM role read models.
//!
//! A role is read as its own scope only, whatever rows it has: a platform role holds platform
//! permissions, and every other role organization permissions. `permissions` are the role's own;
//! `effective_permissions` add those of the policies it attaches.

const ROLE_FIELDS: &str = "role.id, role.org_id, role.role_key, role.name, role.description,
                           role.builtin, role.role_type, role.scope, role.policy_ids,
                           role.created_at, role.updated_at,
                           ARRAY(
                               SELECT granted.permission_key
                                 FROM unnest(iam_role_grants(role.permissions, role.policy_ids))
                                      AS granted(permission_key)
                                 JOIN iam_permissions granted_permission
                                   ON granted_permission.permission_key = granted.permission_key
                                  AND granted_permission.scope =
                                      CASE role.scope WHEN 'platform' THEN 'platform' ELSE 'organization' END
                             ORDER BY granted.permission_key
                           ) AS effective_permissions";

const ROLE_GROUP_BY: &str = "role.id, role.org_id, role.role_key, role.name, role.description,
                            role.builtin, role.role_type, role.scope, role.policy_ids,
                            role.permissions, role.created_at, role.updated_at";

const ROLE_ORDER_BY: &str = "CASE WHEN role.builtin THEN 0 ELSE 1 END,
                             COALESCE(
                                 (SELECT display_priority
                                    FROM iam_roles catalog
                                   WHERE catalog.org_id IS NULL
                                     AND catalog.role_key = role.role_key),
                                 1000
                             ),
                             role.name ASC";

pub(super) fn list_sql() -> String {
    format!(
        "SELECT {ROLE_FIELDS},
                COALESCE(
                    ARRAY_AGG(permission.permission_key ORDER BY permission.permission_key)
                        FILTER (WHERE permission.permission_key IS NOT NULL),
                    ARRAY[]::TEXT[]
                ) AS permissions
           FROM iam_roles role
      LEFT JOIN LATERAL unnest(role.permissions) AS role_permission(permission_key) ON TRUE
      LEFT JOIN iam_permissions permission
             ON permission.permission_key = role_permission.permission_key
            AND permission.scope = CASE role.scope WHEN 'platform' THEN 'platform' ELSE 'organization' END
          WHERE role.org_id = $1
       GROUP BY {ROLE_GROUP_BY}
       ORDER BY {ROLE_ORDER_BY}"
    )
}

pub(super) fn list_with_usage_sql() -> String {
    format!(
        "WITH membership_usage AS (
             SELECT organization_id AS org_id, role_id,
                    COUNT(DISTINCT principal_id) AS memberships
               FROM iam_role_bindings
              WHERE organization_id = $1
                AND principal_type = 'user'
                AND resource_type IS NULL
                AND resource_id IS NULL
           GROUP BY organization_id, role_id
         ), service_account_usage AS (
             SELECT org_id, role_id, COUNT(*) AS service_accounts
               FROM service_accounts
              WHERE org_id = $1 AND deleted_at IS NULL
           GROUP BY org_id, role_id
         ), invitation_usage AS (
             SELECT org_id, role_id, COUNT(*) AS invitations
               FROM invitations
              WHERE org_id = $1 AND status <> 'revoked'
           GROUP BY org_id, role_id
         ), binding_usage AS (
             SELECT organization_id AS org_id, role_id, COUNT(*) AS bindings
               FROM iam_role_bindings
              WHERE organization_id = $1
           GROUP BY organization_id, role_id
         )
         SELECT {ROLE_FIELDS},
                COALESCE(
                    ARRAY_AGG(permission.permission_key ORDER BY permission.permission_key)
                        FILTER (WHERE permission.permission_key IS NOT NULL),
                    ARRAY[]::TEXT[]
                ) AS permissions,
                COALESCE(MAX(membership_usage.memberships), 0) AS memberships,
                COALESCE(MAX(service_account_usage.service_accounts), 0) AS service_accounts,
                COALESCE(MAX(invitation_usage.invitations), 0) AS invitations,
                COALESCE(MAX(binding_usage.bindings), 0) AS bindings
           FROM iam_roles role
      LEFT JOIN LATERAL unnest(role.permissions) AS role_permission(permission_key) ON TRUE
      LEFT JOIN iam_permissions permission
             ON permission.permission_key = role_permission.permission_key
            AND permission.scope = CASE role.scope WHEN 'platform' THEN 'platform' ELSE 'organization' END
      LEFT JOIN membership_usage
             ON membership_usage.org_id = role.org_id
            AND membership_usage.role_id = role.id
      LEFT JOIN service_account_usage
             ON service_account_usage.org_id = role.org_id
            AND service_account_usage.role_id = role.id
      LEFT JOIN invitation_usage
             ON invitation_usage.org_id = role.org_id
            AND invitation_usage.role_id = role.id
      LEFT JOIN binding_usage
             ON binding_usage.org_id = role.org_id
            AND binding_usage.role_id = role.id
          WHERE role.org_id = $1
       GROUP BY {ROLE_GROUP_BY}
       ORDER BY {ROLE_ORDER_BY}"
    )
}

pub(super) fn get_sql() -> String {
    format!(
        "SELECT {ROLE_FIELDS},
                COALESCE(
                    ARRAY_AGG(permission.permission_key ORDER BY permission.permission_key)
                        FILTER (WHERE permission.permission_key IS NOT NULL),
                    ARRAY[]::TEXT[]
                ) AS permissions
           FROM iam_roles role
      LEFT JOIN LATERAL unnest(role.permissions) AS role_permission(permission_key) ON TRUE
      LEFT JOIN iam_permissions permission
             ON permission.permission_key = role_permission.permission_key
            AND permission.scope = CASE role.scope WHEN 'platform' THEN 'platform' ELSE 'organization' END
          WHERE role.org_id = $1 AND role.id = $2
       GROUP BY {ROLE_GROUP_BY}"
    )
}
