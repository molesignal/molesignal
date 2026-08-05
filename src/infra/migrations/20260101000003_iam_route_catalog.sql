-- Database-owned route and navigation access catalog.
--
-- React keeps component/icon bindings, while this catalog is authoritative for
-- route visibility, navigation placement, scopes, features, and permissions.

-- A deployment has exactly one super-administrator: the configured root.
-- Remove the former "last platform administrator" guards before reconciling
-- historical assignments. Startup subsequently selects the configured root
-- under the transaction-local bypass used only by that bootstrap path.
DROP TRIGGER IF EXISTS trg_protect_last_platform_administrator
    ON iam_platform_administrators;
DROP TRIGGER IF EXISTS trg_protect_platform_administrator_user ON users;
DROP FUNCTION IF EXISTS protect_last_platform_administrator();
DROP FUNCTION IF EXISTS protect_platform_administrator_user();

WITH active_assignments AS (
    SELECT user_id,
           ROW_NUMBER() OVER (ORDER BY granted_at_micros, user_id) AS position
      FROM iam_platform_administrators
     WHERE active
)
UPDATE iam_platform_administrators assignment
   SET active = FALSE,
       revoked_by = assignment.user_id,
       revoked_at_micros = (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
  FROM active_assignments active
 WHERE assignment.user_id = active.user_id
   AND active.position > 1;

CREATE UNIQUE INDEX IF NOT EXISTS uq_iam_platform_administrators_single_active
    ON iam_platform_administrators ((active)) WHERE active;

CREATE FUNCTION protect_configured_root_assignment()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF COALESCE(current_setting('molesignal.root_reconcile', TRUE), 'false') = 'true' THEN
        IF TG_OP = 'DELETE' THEN
            RETURN OLD;
        END IF;
        RETURN NEW;
    END IF;
    IF OLD.active THEN
        IF TG_OP = 'DELETE' THEN
            RAISE EXCEPTION 'configured root assignment is immutable';
        END IF;
        IF NEW IS DISTINCT FROM OLD THEN
            RAISE EXCEPTION 'configured root assignment is immutable';
        END IF;
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER trg_protect_configured_root_assignment
BEFORE UPDATE OR DELETE ON iam_platform_administrators
FOR EACH ROW EXECUTE FUNCTION protect_configured_root_assignment();

CREATE FUNCTION protect_configured_root_user()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    is_root BOOLEAN;
BEGIN
    SELECT EXISTS (
        SELECT 1
          FROM iam_platform_administrators
         WHERE user_id = OLD.id
           AND active
    ) INTO is_root;
    IF is_root THEN
        IF TG_OP = 'DELETE' THEN
            RAISE EXCEPTION 'configured root user is immutable and must remain active';
        END IF;
        IF NEW.id IS DISTINCT FROM OLD.id
            OR NEW.email IS DISTINCT FROM OLD.email
            OR NEW.disabled
            OR NEW.status IS DISTINCT FROM 'active' THEN
            RAISE EXCEPTION 'configured root user is immutable and must remain active';
        END IF;
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER trg_protect_configured_root_user
BEFORE UPDATE OR DELETE ON users
FOR EACH ROW EXECUTE FUNCTION protect_configured_root_user();

CREATE TABLE IF NOT EXISTS iam_route_catalog_versions (
    catalog_key         VARCHAR(64) PRIMARY KEY,
    version             BIGINT      NOT NULL CHECK (version > 0),
    updated_at_micros   BIGINT      NOT NULL
);

CREATE TABLE IF NOT EXISTS iam_routes (
    route_key            VARCHAR(128) PRIMARY KEY,
    path_pattern         VARCHAR(255) NOT NULL UNIQUE,
    scope                VARCHAR(16)  NOT NULL,
    permission_mode      VARCHAR(8)   NOT NULL DEFAULT 'all',
    required_features    TEXT[]       NOT NULL DEFAULT ARRAY[]::TEXT[],
    navigation_group     VARCHAR(32),
    navigation_position INTEGER,
    enabled              BOOLEAN      NOT NULL DEFAULT TRUE,
    catalog_version      BIGINT       NOT NULL,
    CONSTRAINT chk_iam_routes_key_format
        CHECK (route_key ~ '^[a-z0-9_]+(\.[a-z0-9_]+)*$'),
    CONSTRAINT chk_iam_routes_scope
        CHECK (scope IN ('any', 'organization', 'system', 'none')),
    CONSTRAINT chk_iam_routes_permission_mode
        CHECK (permission_mode IN ('all', 'any')),
    CONSTRAINT chk_iam_routes_navigation
        CHECK (
            (navigation_group IS NULL AND navigation_position IS NULL)
            OR (
                navigation_group IN ('home', 'investigate', 'pipeline', 'admin')
                AND navigation_position >= 0
            )
        )
);

CREATE TABLE IF NOT EXISTS iam_route_permissions (
    route_key       VARCHAR(128) NOT NULL
        REFERENCES iam_routes(route_key) ON DELETE CASCADE,
    permission_key  VARCHAR(128) NOT NULL
        REFERENCES iam_permissions(permission_key) ON DELETE RESTRICT,
    position        INTEGER      NOT NULL CHECK (position >= 0),
    PRIMARY KEY (route_key, permission_key)
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_iam_route_permission_position
    ON iam_route_permissions (route_key, position);

INSERT INTO iam_route_catalog_versions (catalog_key, version, updated_at_micros)
VALUES ('routes', 2, (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT)
ON CONFLICT (catalog_key) DO UPDATE
SET version = EXCLUDED.version,
    updated_at_micros = EXCLUDED.updated_at_micros;

CREATE TEMP TABLE iam_route_seed (
    route_key            VARCHAR(128) NOT NULL,
    path_pattern         VARCHAR(255) NOT NULL,
    scope                VARCHAR(16)  NOT NULL,
    permission_mode      VARCHAR(8)   NOT NULL,
    required_features    TEXT[]       NOT NULL,
    navigation_group     VARCHAR(32),
    navigation_position INTEGER,
    enabled              BOOLEAN      NOT NULL,
    permissions          TEXT[]       NOT NULL
) ON COMMIT DROP;

INSERT INTO iam_route_seed VALUES
    -- Personal account routes are available in either interactive scope.
    ('root', '/', 'any', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY[]::TEXT[]),
    ('account.settings', '/account/settings', 'any', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY[]::TEXT[]),
    ('account.settings.section', '/account/settings/:section', 'any', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY[]::TEXT[]),

    -- Read-only observability routes shared by tenant and system scope.
    ('home', '/home', 'any', 'any', ARRAY[]::TEXT[], 'home', 10, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('metrics', '/metrics', 'any', 'any', ARRAY[]::TEXT[], 'investigate', 20, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('logs', '/logs', 'any', 'any', ARRAY[]::TEXT[], 'investigate', 30, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('logs.inspector', '/logs/inspector', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('traces', '/traces', 'any', 'any', ARRAY[]::TEXT[], 'investigate', 40, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('trace.session.detail', '/traces/session/:id', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('trace.detail', '/traces/:id', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('service.graph', '/service-graph', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('profiles', '/profiles', 'any', 'any', ARRAY[]::TEXT[], 'investigate', 70, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('profiles.compare', '/profiles/compare', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('profile.detail', '/profiles/:id', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('services.legacy', '/services', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('service.legacy.detail', '/services/:service', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),

    -- APM routes.
    ('apm', '/apm', 'any', 'any', ARRAY[]::TEXT[], 'investigate', 50, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('apm.overview', '/apm/overview', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('apm.services', '/apm/services', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('apm.service.detail', '/apm/services/:service', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('apm.service.runtime', '/apm/services/:service/runtime', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('apm.transactions', '/apm/transactions', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('apm.transaction.detail', '/apm/transactions/:transaction', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('apm.dependencies', '/apm/dependencies', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('apm.errors', '/apm/errors', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('apm.error.detail', '/apm/errors/:fingerprint', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('apm.deployments', '/apm/deployments', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('apm.version.compare', '/apm/versions/compare', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('apm.user.experience', '/apm/user-experience/*', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),

    -- Dashboards and stream catalog are readable in both scopes.
    ('dashboards', '/dashboards', 'any', 'any', ARRAY[]::TEXT[], 'investigate', 10, TRUE, ARRAY['dashboards.read', 'sys.dashboards.read']::TEXT[]),
    ('dashboard.detail', '/dashboards/:id', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['dashboards.read', 'sys.dashboards.read']::TEXT[]),
    ('dashboard.new.edit', '/dashboards/new/edit', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['dashboards.create']::TEXT[]),
    ('dashboard.import', '/dashboards/import', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['dashboards.create']::TEXT[]),
    ('dashboard.edit', '/dashboards/:id/edit', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['dashboards.edit']::TEXT[]),
    ('dashboard.new.panel', '/dashboards/:id/panels/new', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['dashboards.edit']::TEXT[]),
    ('streams', '/streams', 'any', 'any', ARRAY[]::TEXT[], 'pipeline', 20, TRUE, ARRAY['streams.read', 'sys.telemetry.read']::TEXT[]),
    ('stream.explore', '/streams/:id', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.read', 'sys.telemetry.read']::TEXT[]),

    -- Tenant-only RUM routes.
    ('rum', '/rum', 'organization', 'all', ARRAY[]::TEXT[], 'investigate', 60, TRUE, ARRAY['streams.query']::TEXT[]),
    ('rum.overview', '/rum/overview', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('rum.applications', '/rum/applications', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('rum.sessions', '/rum/sessions', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('rum.session', '/rum/sessions/view/:id', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('rum.pages', '/rum/pages', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('rum.errors', '/rum/errors', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('rum.error', '/rum/errors/view/:id', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('rum.performance', '/rum/performance/*', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('rum.session.replay', '/rum/session-replay', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('rum.settings', '/rum/settings/:section', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('rum.source.maps', '/rum/settings/source-maps', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.read']::TEXT[]),
    ('rum.upload.source.maps', '/rum/settings/source-maps/upload', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.configure']::TEXT[]),
    ('rum.source.maps.legacy', '/rum/source-maps', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.read']::TEXT[]),
    ('rum.upload.source.maps.legacy', '/rum/upload-source-maps', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.configure']::TEXT[]),

    -- Tenant observability and data-management routes.
    ('datasource', '/datasource', 'organization', 'all', ARRAY[]::TEXT[], 'pipeline', 10, TRUE, ARRAY['streams.query']::TEXT[]),
    ('datasource.category', '/datasource/:category', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('datasource.source', '/datasource/:category/:source', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('datasources.legacy', '/datasources', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('ingest', '/ingest', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('ingest.category', '/ingest/:category', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('ingest.source', '/ingest/:category/:source', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('alerts', '/alerts', 'organization', 'all', ARRAY[]::TEXT[], 'investigate', 80, TRUE, ARRAY['alerts.read']::TEXT[]),
    ('alert.incidents', '/alerts/incidents', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['alerts.read']::TEXT[]),
    ('alert.rules', '/alerts/rules', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['alerts.read']::TEXT[]),
    ('alert.rule.new', '/alerts/rules/new', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['alerts.manage']::TEXT[]),
    ('alert.rule.edit', '/alerts/rules/:id/edit', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['alerts.manage']::TEXT[]),
    ('incident.detail', '/alerts/incidents/:id', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['alerts.read']::TEXT[]),
    ('anomaly.new', '/alerts/anomaly/add', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['alerts.manage']::TEXT[]),
    ('anomaly.edit', '/alerts/anomaly/edit/:id', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['alerts.manage']::TEXT[]),
    ('semantic.groups', '/alerts/semantic-groups', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['alerts.manage']::TEXT[]),
    ('import.semantic.groups', '/alerts/import-semantic-groups', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['alerts.manage']::TEXT[]),
    ('alert.read.tools', '/alerts/:section', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['alerts.read']::TEXT[]),
    ('alert.schedule.detail', '/alerts/schedules/:id', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['schedules.read']::TEXT[]),
    ('saved.views', '/saved-views', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['saved_views.read']::TEXT[]),
    ('pipelines', '/pipelines', 'organization', 'all', ARRAY[]::TEXT[], 'pipeline', 30, TRUE, ARRAY['pipelines.read']::TEXT[]),
    ('pipeline.detail', '/pipelines/:id', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['pipelines.read']::TEXT[]),
    ('pipeline.add', '/pipelines/new', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['pipelines.create']::TEXT[]),
    ('pipeline.import', '/pipelines/import', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['pipelines.create']::TEXT[]),
    ('pipeline.connectors', '/pipelines/connectors', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['pipelines.read']::TEXT[]),
    ('pipeline.edit', '/pipelines/:id/edit', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['pipelines.edit']::TEXT[]),
    ('pipeline.history', '/pipelines/:id/history', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['pipelines.read']::TEXT[]),
    ('pipeline.backfill', '/pipelines/:id/backfill', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['pipelines.run']::TEXT[]),
    ('functions', '/functions', 'organization', 'all', ARRAY[]::TEXT[], 'pipeline', 40, TRUE, ARRAY['functions.read']::TEXT[]),
    ('function.new', '/functions/new', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['functions.create']::TEXT[]),
    ('function.detail', '/functions/:id', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['functions.read']::TEXT[]),
    ('extend.tables', '/extend-tables', 'organization', 'all', ARRAY[]::TEXT[], 'pipeline', 50, TRUE, ARRAY['functions.read']::TEXT[]),
    ('extend.table.detail', '/extend-tables/:table', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['functions.read']::TEXT[]),
    ('reports', '/reports', 'organization', 'all', ARRAY[]::TEXT[], 'pipeline', 60, TRUE, ARRAY['reports.read']::TEXT[]),
    ('investigate', '/investigate', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),

    -- Intelligence is permission- and license-gated in the database catalog.
    ('intelligence', '/intelligence', 'organization', 'all', ARRAY['intelligence']::TEXT[], 'investigate', 90, TRUE, ARRAY['intelligence.use']::TEXT[]),
    ('intelligence.section', '/intelligence/:section', 'organization', 'all', ARRAY['intelligence']::TEXT[], NULL, NULL, TRUE, ARRAY['intelligence.use']::TEXT[]),
    ('intelligence.detail', '/intelligence/:section/:id', 'organization', 'all', ARRAY['intelligence']::TEXT[], NULL, NULL, TRUE, ARRAY['intelligence.use']::TEXT[]),

    -- Organization IAM and settings.
    ('iam', '/iam', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.members.read', 'sys.organizations.manage']::TEXT[]),
    ('iam.users', '/iam/users', 'organization', 'all', ARRAY[]::TEXT[], 'admin', 10, TRUE, ARRAY['org.members.read']::TEXT[]),
    ('iam.invitations', '/iam/invitations', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.members.read']::TEXT[]),
    ('iam.approvals', '/iam/approvals', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.members.read']::TEXT[]),
    ('iam.teams', '/iam/teams', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.members.read']::TEXT[]),
    ('iam.roles', '/iam/roles', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['iam.roles.read']::TEXT[]),
    ('iam.groups', '/iam/groups', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['iam.policies.read']::TEXT[]),
    ('iam.service.accounts', '/iam/service-accounts', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['api_tokens.read']::TEXT[]),
    ('iam.sso', '/iam/sso', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.settings.read']::TEXT[]),
    ('iam.email.domains', '/iam/email-domains', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.settings.read']::TEXT[]),
    ('iam.quota', '/iam/quota', 'none', 'all', ARRAY[]::TEXT[], NULL, NULL, FALSE, ARRAY[]::TEXT[]),
    ('settings', '/settings', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.settings.read', 'sys.organizations.manage']::TEXT[]),
    ('settings.general', '/settings/general', 'organization', 'all', ARRAY[]::TEXT[], 'admin', 20, TRUE, ARRAY['org.settings.read']::TEXT[]),
    ('settings.organization', '/settings/organization', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.settings.read']::TEXT[]),
    ('settings.billing', '/settings/billing', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.billing.read']::TEXT[]),
    ('settings.correlation', '/settings/correlation', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('settings.audit', '/settings/audit', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['audit.read']::TEXT[]),
    ('settings.notify', '/settings/notify/*', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['alerts.read']::TEXT[]),
    ('settings.sso.providers', '/settings/sso_providers', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.settings.read']::TEXT[]),
    ('settings.tenant.tools', '/settings/:section', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.settings.read']::TEXT[]),
    ('account.billing', '/account/billing', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.billing.read']::TEXT[]),
    ('account.support', '/account/support', 'organization', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.settings.read']::TEXT[]),

    -- Root-only system IAM and settings.
    ('iam.organizations', '/iam/organizations', 'system', 'all', ARRAY[]::TEXT[], 'admin', 10, TRUE, ARRAY['sys.organizations.manage']::TEXT[]),
    ('settings.organization.management', '/settings/organization_management', 'system', 'all', ARRAY[]::TEXT[], 'admin', 20, TRUE, ARRAY['sys.organizations.manage']::TEXT[]),
    ('settings.license', '/settings/license', 'system', 'all', ARRAY[]::TEXT[], 'admin', 30, TRUE, ARRAY['sys.licenses.read']::TEXT[]),
    ('settings.client_ip', '/settings/client_ip', 'system', 'all', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['sys.settings.manage']::TEXT[]),
    ('settings.model.pricing', '/settings/model_pricing', 'any', 'any', ARRAY[]::TEXT[], NULL, NULL, TRUE, ARRAY['org.settings.read', 'sys.settings.manage']::TEXT[]);

DELETE FROM iam_route_permissions;
DELETE FROM iam_routes route
WHERE NOT EXISTS (
    SELECT 1 FROM iam_route_seed seed WHERE seed.route_key = route.route_key
);

INSERT INTO iam_routes (
    route_key,
    path_pattern,
    scope,
    permission_mode,
    required_features,
    navigation_group,
    navigation_position,
    enabled,
    catalog_version
)
SELECT
    route_key,
    path_pattern,
    scope,
    permission_mode,
    required_features,
    navigation_group,
    navigation_position,
    enabled,
    2
FROM iam_route_seed
ON CONFLICT (route_key) DO UPDATE
SET path_pattern = EXCLUDED.path_pattern,
    scope = EXCLUDED.scope,
    permission_mode = EXCLUDED.permission_mode,
    required_features = EXCLUDED.required_features,
    navigation_group = EXCLUDED.navigation_group,
    navigation_position = EXCLUDED.navigation_position,
    enabled = EXCLUDED.enabled,
    catalog_version = EXCLUDED.catalog_version;

INSERT INTO iam_route_permissions (route_key, permission_key, position)
SELECT
    seed.route_key,
    item.permission_key,
    item.position - 1
FROM iam_route_seed seed
CROSS JOIN LATERAL unnest(seed.permissions)
    WITH ORDINALITY AS item(permission_key, position);

CREATE OR REPLACE FUNCTION bump_iam_route_catalog_version()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE iam_route_catalog_versions
       SET version = version + 1,
           updated_at_micros = (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
     WHERE catalog_key = 'routes';
    RETURN NULL;
END
$$;

DROP TRIGGER IF EXISTS trg_iam_routes_catalog_version ON iam_routes;
CREATE TRIGGER trg_iam_routes_catalog_version
AFTER INSERT OR UPDATE OR DELETE ON iam_routes
FOR EACH STATEMENT EXECUTE FUNCTION bump_iam_route_catalog_version();

DROP TRIGGER IF EXISTS trg_iam_route_permissions_catalog_version ON iam_route_permissions;
CREATE TRIGGER trg_iam_route_permissions_catalog_version
AFTER INSERT OR UPDATE OR DELETE ON iam_route_permissions
FOR EACH STATEMENT EXECUTE FUNCTION bump_iam_route_catalog_version();
