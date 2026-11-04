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
           ROW_NUMBER() OVER (ORDER BY granted_at, user_id) AS position
      FROM iam_platform_administrators
     WHERE active
)
UPDATE iam_platform_administrators assignment
   SET active = FALSE,
       revoked_by = assignment.user_id,
       revoked_at = clock_timestamp()
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

-- A route lists the permissions it needs in order; its catalog version is the
-- 'routes' row of iam_catalog_versions.
CREATE TABLE IF NOT EXISTS iam_routes (
    route_key            VARCHAR(128) PRIMARY KEY,
    path_pattern         VARCHAR(255) NOT NULL UNIQUE,
    scope                VARCHAR(16)  NOT NULL,
    permission_mode      VARCHAR(8)   NOT NULL DEFAULT 'all',
    navigation_group     VARCHAR(32),
    navigation_position INTEGER,
    enabled              BOOLEAN      NOT NULL DEFAULT TRUE,
    catalog_version      BIGINT       NOT NULL,
    permissions          TEXT[]       NOT NULL DEFAULT ARRAY[]::TEXT[],
    CONSTRAINT chk_iam_routes_permissions
        CHECK (array_position(permissions, NULL) IS NULL),
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
                navigation_group IN (
                    'home', 'observe', 'reliability', 'ai_ops',
                    'data', 'collaboration', 'admin'
                )
                AND navigation_position >= 0
            )
        )
);

-- Every permission a route names must be in the catalog.
DROP TRIGGER IF EXISTS trg_iam_routes_permissions_known ON iam_routes;
CREATE TRIGGER trg_iam_routes_permissions_known
BEFORE INSERT OR UPDATE OF permissions ON iam_routes
FOR EACH ROW EXECUTE FUNCTION check_iam_permission_keys();

INSERT INTO iam_catalog_versions (catalog_key, version, updated_at)
VALUES ('routes', 2, clock_timestamp())
ON CONFLICT (catalog_key) DO UPDATE
SET version = EXCLUDED.version,
    updated_at = EXCLUDED.updated_at;

CREATE TEMP TABLE iam_route_seed (
    route_key            VARCHAR(128) NOT NULL,
    path_pattern         VARCHAR(255) NOT NULL,
    scope                VARCHAR(16)  NOT NULL,
    permission_mode      VARCHAR(8)   NOT NULL,
    navigation_group     VARCHAR(32),
    navigation_position INTEGER,
    enabled              BOOLEAN      NOT NULL,
    permissions          TEXT[]       NOT NULL
) ON COMMIT DROP;

INSERT INTO iam_route_seed VALUES
    -- Personal account routes are available in either interactive scope.
    ('root', '/', 'any', 'all', NULL, NULL, TRUE, ARRAY[]::TEXT[]),
    ('account.settings', '/account/settings', 'any', 'all', NULL, NULL, TRUE, ARRAY[]::TEXT[]),
    ('account.settings.section', '/account/settings/:section', 'any', 'all', NULL, NULL, TRUE, ARRAY[]::TEXT[]),

    -- Read-only observability routes shared by tenant and system scope.
    ('home', '/home', 'any', 'any', 'home', 10, TRUE, ARRAY['streams.query', 'sys.telemetry.read']::TEXT[]),
    ('metrics', '/metrics', 'any', 'any', 'observe', 20, TRUE, ARRAY['metrics.read', 'sys.telemetry.read']::TEXT[]),
    ('logs', '/logs', 'any', 'any', 'observe', 30, TRUE, ARRAY['logs.read', 'sys.telemetry.read']::TEXT[]),
    ('logs.inspector', '/logs/inspector', 'any', 'any', NULL, NULL, TRUE, ARRAY['logs.read', 'sys.telemetry.read']::TEXT[]),
    ('traces', '/traces', 'any', 'any', 'observe', 40, TRUE, ARRAY['traces.read', 'sys.telemetry.read']::TEXT[]),
    ('trace.session.detail', '/traces/session/:id', 'any', 'any', NULL, NULL, TRUE, ARRAY['traces.read', 'sys.telemetry.read']::TEXT[]),
    ('trace.detail', '/traces/:id', 'any', 'any', NULL, NULL, TRUE, ARRAY['traces.read', 'sys.telemetry.read']::TEXT[]),
    ('service.graph', '/service-graph', 'any', 'any', NULL, NULL, TRUE, ARRAY['traces.read', 'sys.telemetry.read']::TEXT[]),
    ('profiles', '/profiles', 'any', 'any', 'observe', 70, TRUE, ARRAY['profiles.read', 'sys.telemetry.read']::TEXT[]),
    ('profiles.compare', '/profiles/compare', 'any', 'any', NULL, NULL, TRUE, ARRAY['profiles.read', 'sys.telemetry.read']::TEXT[]),
    ('profile.detail', '/profiles/:id', 'any', 'any', NULL, NULL, TRUE, ARRAY['profiles.read', 'sys.telemetry.read']::TEXT[]),
    ('services.legacy', '/services', 'any', 'any', NULL, NULL, TRUE, ARRAY['apm.services.read', 'sys.telemetry.read']::TEXT[]),
    ('service.legacy.detail', '/services/:service', 'any', 'any', NULL, NULL, TRUE, ARRAY['apm.services.read', 'sys.telemetry.read']::TEXT[]),

    -- APM routes.
    ('apm', '/apm', 'any', 'any', 'observe', 50, TRUE, ARRAY['apm.services.read', 'apm.errors.read', 'apm.slos.read', 'apm.settings.read', 'sys.telemetry.read']::TEXT[]),
    ('apm.overview', '/apm/overview', 'any', 'any', NULL, NULL, TRUE, ARRAY['apm.services.read', 'sys.telemetry.read']::TEXT[]),
    ('apm.services', '/apm/services', 'any', 'any', NULL, NULL, TRUE, ARRAY['apm.services.read', 'sys.telemetry.read']::TEXT[]),
    ('apm.slos', '/apm/slos', 'any', 'any', NULL, NULL, TRUE, ARRAY['apm.slos.read', 'sys.telemetry.read']::TEXT[]),
    ('apm.map', '/apm/map', 'any', 'any', NULL, NULL, TRUE, ARRAY['apm.services.read', 'sys.telemetry.read']::TEXT[]),
    ('apm.errors', '/apm/errors', 'any', 'any', NULL, NULL, TRUE, ARRAY['apm.errors.read', 'sys.telemetry.read']::TEXT[]),
    ('apm.settings', '/apm/settings', 'any', 'any', NULL, NULL, TRUE, ARRAY['apm.settings.read', 'sys.telemetry.read']::TEXT[]),
    ('apm.service.detail', '/apm/services/:service', 'any', 'any', NULL, NULL, TRUE, ARRAY['apm.services.read', 'sys.telemetry.read']::TEXT[]),
    ('apm.service.section', '/apm/services/:service/:section', 'any', 'any', NULL, NULL, TRUE, ARRAY['apm.services.read', 'sys.telemetry.read']::TEXT[]),
    ('apm.user.experience', '/apm/user-experience/*', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.events.read']::TEXT[]),

    -- Dashboards and stream catalog are readable in both scopes.
    ('dashboards', '/dashboards', 'any', 'any', 'observe', 10, TRUE, ARRAY['dashboards.read', 'sys.dashboards.read']::TEXT[]),
    ('dashboard.detail', '/dashboards/:id', 'any', 'any', NULL, NULL, TRUE, ARRAY['dashboards.read', 'sys.dashboards.read']::TEXT[]),
    ('dashboard.new.edit', '/dashboards/new/edit', 'organization', 'all', NULL, NULL, TRUE, ARRAY['dashboards.create']::TEXT[]),
    ('dashboard.import', '/dashboards/import', 'organization', 'all', NULL, NULL, TRUE, ARRAY['dashboards.create']::TEXT[]),
    ('dashboard.edit', '/dashboards/:id/edit', 'organization', 'all', NULL, NULL, TRUE, ARRAY['dashboards.edit']::TEXT[]),
    ('dashboard.new.panel', '/dashboards/:id/panels/new', 'organization', 'all', NULL, NULL, TRUE, ARRAY['dashboards.edit']::TEXT[]),
    ('streams', '/streams', 'any', 'any', 'data', 20, TRUE, ARRAY['streams.read', 'sys.telemetry.read']::TEXT[]),
    ('stream.usage', '/streams/usage', 'any', 'any', NULL, NULL, TRUE, ARRAY['intake_usage.read', 'sys.telemetry.read']::TEXT[]),
    ('stream.explore', '/streams/:id', 'any', 'any', NULL, NULL, TRUE, ARRAY['streams.read', 'sys.telemetry.read']::TEXT[]),

    -- Tenant-only RUM routes.
    ('rum', '/rum', 'organization', 'any', 'observe', 60, TRUE, ARRAY['rum.events.read', 'rum.playlists.read', 'rum.issues.read', 'rum.analytics.read', 'rum.applications.read']::TEXT[]),
    ('rum.overview', '/rum/overview', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.events.read']::TEXT[]),
    ('rum.explorer', '/rum/explorer', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.events.read']::TEXT[]),
    ('rum.errors', '/rum/errors', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.issues.read']::TEXT[]),
    ('rum.performance', '/rum/performance', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.events.read']::TEXT[]),
    ('rum.performance.resources', '/rum/performance/resources', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.events.read']::TEXT[]),
    ('rum.performance.long.tasks', '/rum/performance/long-tasks', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.events.read']::TEXT[]),
    ('rum.frustration', '/rum/frustration', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.events.read']::TEXT[]),
    ('rum.analytics', '/rum/analytics', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.analytics.read']::TEXT[]),
    ('rum.analytics.paths', '/rum/analytics/paths', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.analytics.read']::TEXT[]),
    ('rum.analytics.retention', '/rum/analytics/retention', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.analytics.read']::TEXT[]),
    ('rum.analytics.heatmaps', '/rum/analytics/heatmaps', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.analytics.read']::TEXT[]),
    ('rum.feature.flags', '/rum/feature-flags', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.events.read']::TEXT[]),
    ('rum.applications', '/rum/applications', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.applications.read']::TEXT[]),
    ('rum.session', '/rum/sessions/view/:id', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.sessions.read']::TEXT[]),
    ('rum.playlists', '/rum/playlists', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.playlists.read']::TEXT[]),
    ('rum.playlist', '/rum/playlists/:id', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.playlists.read']::TEXT[]),
    ('rum.settings', '/rum/settings/:section', 'organization', 'all', NULL, NULL, TRUE, ARRAY['rum.applications.read']::TEXT[]),
    ('rum.source.maps', '/rum/settings/source-maps', 'organization', 'all', NULL, NULL, TRUE, ARRAY['debug_artifacts.read']::TEXT[]),
    ('rum.upload.source.maps', '/rum/settings/source-maps/upload', 'organization', 'all', NULL, NULL, TRUE, ARRAY['debug_artifacts.manage']::TEXT[]),
    ('rum.source.maps.legacy', '/rum/source-maps', 'organization', 'all', NULL, NULL, TRUE, ARRAY['debug_artifacts.read']::TEXT[]),
    ('rum.upload.source.maps.legacy', '/rum/upload-source-maps', 'organization', 'all', NULL, NULL, TRUE, ARRAY['debug_artifacts.manage']::TEXT[]),

    -- Tenant observability and data-management routes.
    ('datasource', '/datasource', 'organization', 'all', 'data', 10, TRUE, ARRAY['streams.query']::TEXT[]),
    ('datasource.category', '/datasource/:category', 'organization', 'all', NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('datasource.source', '/datasource/:category/:source', 'organization', 'all', NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('datasources.legacy', '/datasources', 'organization', 'all', NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('intake', '/intake', 'organization', 'all', NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('intake.category', '/intake/:category', 'organization', 'all', NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('intake.source', '/intake/:category/:source', 'organization', 'all', NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),
    ('alerts', '/alerts', 'organization', 'any', 'reliability', 10, TRUE, ARRAY['alerts.incidents.read', 'alerts.rules.read', 'alerts.discovery.read', 'alerts.sources.read', 'postmortems.read', 'alerts.mutes.read', 'alerts.inhibitions.read', 'alerts.routing.read', 'alerts.escalations.read', 'schedules.read', 'alerts.remediation.read', 'alerts.tickets.read', 'alerts.semantic_groups.manage']::TEXT[]),
    ('synthetics', '/synthetics/*', 'organization', 'any', 'reliability', 20, TRUE, ARRAY['synthetics.monitors.read', 'synthetics.results.read', 'synthetics.locations.read', 'synthetics.secrets.read']::TEXT[]),
    ('synthetics.overview', '/synthetics/overview', 'organization', 'all', NULL, NULL, TRUE, ARRAY['synthetics.monitors.read']::TEXT[]),
    ('synthetics.checks', '/synthetics/checks', 'organization', 'all', NULL, NULL, TRUE, ARRAY['synthetics.monitors.read']::TEXT[]),
    ('synthetics.results', '/synthetics/results', 'organization', 'all', NULL, NULL, TRUE, ARRAY['synthetics.results.read']::TEXT[]),
    ('synthetics.locations', '/synthetics/locations', 'organization', 'all', NULL, NULL, TRUE, ARRAY['synthetics.locations.read']::TEXT[]),
    ('synthetics.agents', '/synthetics/agents', 'organization', 'all', NULL, NULL, TRUE, ARRAY['synthetics.locations.read']::TEXT[]),
    ('synthetics.variables', '/synthetics/variables', 'organization', 'all', NULL, NULL, TRUE, ARRAY['synthetics.secrets.read']::TEXT[]),
    ('status.pages', '/status-pages', 'organization', 'all', 'reliability', 30, TRUE, ARRAY['status_pages.read']::TEXT[]),
    ('alert.incidents', '/alerts/incidents', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.incidents.read']::TEXT[]),
    ('alert.rules', '/alerts/rules', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.rules.read']::TEXT[]),
    ('alert.rule.new', '/alerts/rules/new', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.rules.manage']::TEXT[]),
    ('alert.rule.edit', '/alerts/rules/:id/edit', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.rules.manage']::TEXT[]),
    ('incident.detail', '/alerts/incidents/:id', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.incidents.read']::TEXT[]),
    ('anomaly.new', '/alerts/anomaly/add', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.rules.manage']::TEXT[]),
    ('anomaly.edit', '/alerts/anomaly/edit/:id', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.rules.manage']::TEXT[]),
    ('semantic.groups', '/alerts/semantic-groups', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.semantic_groups.manage']::TEXT[]),
    ('import.semantic.groups', '/alerts/import-semantic-groups', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.semantic_groups.manage']::TEXT[]),
    ('alert.insights', '/alerts/insights', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.incidents.read']::TEXT[]),
    ('alert.history', '/alerts/history', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.incidents.read']::TEXT[]),
    ('alert.discovery', '/alerts/discovery', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.discovery.read']::TEXT[]),
    ('alert.sources', '/alerts/sources', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.sources.read']::TEXT[]),
    ('alert.silences', '/alerts/silences', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.mutes.read']::TEXT[]),
    ('alert.inhibitions', '/alerts/inhibitions', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.inhibitions.read']::TEXT[]),
    ('alert.routing', '/alerts/routing', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.routing.read']::TEXT[]),
    ('alert.escalations', '/alerts/escalations', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.escalations.read']::TEXT[]),
    ('alert.schedules', '/alerts/schedules', 'organization', 'all', NULL, NULL, TRUE, ARRAY['schedules.read']::TEXT[]),
    ('alert.remediation', '/alerts/remediation', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.remediation.read']::TEXT[]),
    ('alert.tickets', '/alerts/tickets', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.tickets.read']::TEXT[]),
    ('alert.schedule.detail', '/alerts/schedules/:id', 'organization', 'all', NULL, NULL, TRUE, ARRAY['schedules.read']::TEXT[]),
    ('alert.source.detail', '/alerts/sources/:id', 'organization', 'all', NULL, NULL, TRUE, ARRAY['alerts.sources.read']::TEXT[]),
    ('alerts.postmortems', '/alerts/postmortems', 'organization', 'all', NULL, NULL, TRUE, ARRAY['postmortems.read']::TEXT[]),
    ('saved.views', '/saved-views', 'organization', 'all', NULL, NULL, TRUE, ARRAY['saved_views.read']::TEXT[]),
    ('pipelines', '/pipelines', 'organization', 'all', 'data', 30, TRUE, ARRAY['pipelines.read']::TEXT[]),
    ('pipeline.detail', '/pipelines/:id', 'organization', 'all', NULL, NULL, TRUE, ARRAY['pipelines.read']::TEXT[]),
    ('pipeline.add', '/pipelines/new', 'organization', 'all', NULL, NULL, TRUE, ARRAY['pipelines.create']::TEXT[]),
    ('pipeline.import', '/pipelines/import', 'organization', 'all', NULL, NULL, TRUE, ARRAY['pipelines.create']::TEXT[]),
    ('pipeline.connectors', '/pipelines/connectors', 'organization', 'all', NULL, NULL, TRUE, ARRAY['connectors.read']::TEXT[]),
    ('pipeline.edit', '/pipelines/:id/edit', 'organization', 'all', NULL, NULL, TRUE, ARRAY['pipelines.edit']::TEXT[]),
    ('pipeline.history', '/pipelines/:id/history', 'organization', 'all', NULL, NULL, TRUE, ARRAY['pipelines.read']::TEXT[]),
    ('pipeline.backfill', '/pipelines/:id/backfill', 'organization', 'all', NULL, NULL, TRUE, ARRAY['pipelines.run']::TEXT[]),
    ('functions', '/functions', 'organization', 'all', 'data', 40, TRUE, ARRAY['functions.read']::TEXT[]),
    ('function.new', '/functions/new', 'organization', 'all', NULL, NULL, TRUE, ARRAY['functions.create']::TEXT[]),
    ('function.detail', '/functions/:id', 'organization', 'all', NULL, NULL, TRUE, ARRAY['functions.read']::TEXT[]),
    ('extend.tables', '/extend-tables', 'organization', 'all', 'data', 50, TRUE, ARRAY['tables.read']::TEXT[]),
    ('extend.table.detail', '/extend-tables/:table', 'organization', 'all', NULL, NULL, TRUE, ARRAY['tables.read']::TEXT[]),
    ('reports', '/reports', 'organization', 'all', 'collaboration', 10, TRUE, ARRAY['reports.read']::TEXT[]),
    ('reports.service_levels', '/reports/sla-policies', 'organization', 'all', NULL, NULL, TRUE, ARRAY['service_levels.read']::TEXT[]),
    ('investigate', '/investigate', 'organization', 'all', NULL, NULL, TRUE, ARRAY['streams.query']::TEXT[]),

    -- Agent is permission-gated in the database catalog.
    ('agent', '/agent', 'organization', 'all', 'ai_ops', 10, TRUE, ARRAY['agent.use']::TEXT[]),
    ('agent.section', '/agent/:section', 'organization', 'all', NULL, NULL, TRUE, ARRAY['agent.use']::TEXT[]),
    ('agent.detail', '/agent/:section/:id', 'organization', 'all', NULL, NULL, TRUE, ARRAY['agent.use']::TEXT[]),
    -- Scheduled log inspections are run by Mole Agent and their reports quote logs:
    -- reading them takes both permissions. They are reached from the Logs page.
    ('logs.inspections', '/logs/inspections', 'organization', 'all', NULL, NULL, TRUE, ARRAY['inspections.read', 'logs.read']::TEXT[]),
    ('logs.inspection.detail', '/logs/inspections/:id', 'organization', 'all', NULL, NULL, TRUE, ARRAY['inspections.read', 'logs.read']::TEXT[]),

    -- Organization IAM and settings.
    ('iam', '/iam', 'any', 'any', 'admin', 10, TRUE, ARRAY['org.members.read', 'iam.roles.read', 'iam.policies.read', 'iam.grants.read', 'api_tokens.read', 'service_accounts.read', 'org.sso.read', 'org.email_domains.read', 'sys.organizations.read', 'sys.administrators.read']::TEXT[]),
    ('iam.users', '/iam/users', 'organization', 'all', NULL, NULL, TRUE, ARRAY['org.members.read']::TEXT[]),
    ('iam.invitations', '/iam/invitations', 'organization', 'all', NULL, NULL, TRUE, ARRAY['org.members.read']::TEXT[]),
    ('iam.approvals', '/iam/approvals', 'organization', 'all', NULL, NULL, TRUE, ARRAY['org.members.read']::TEXT[]),
    ('iam.teams', '/iam/teams', 'organization', 'all', NULL, NULL, TRUE, ARRAY['org.members.read']::TEXT[]),
    ('iam.roles', '/iam/roles', 'any', 'any', NULL, NULL, TRUE, ARRAY['iam.roles.read', 'sys.administrators.read']::TEXT[]),
    ('iam.permission.policies', '/iam/policies', 'any', 'any', NULL, NULL, TRUE, ARRAY['iam.policies.read', 'sys.administrators.read']::TEXT[]),
    ('iam.groups', '/iam/groups', 'any', 'any', NULL, NULL, TRUE, ARRAY['iam.grants.read', 'sys.administrators.read']::TEXT[]),
    ('iam.api.tokens', '/iam/api-tokens', 'organization', 'all', NULL, NULL, TRUE, ARRAY['api_tokens.read']::TEXT[]),
    ('iam.service.accounts', '/iam/service-accounts', 'organization', 'all', NULL, NULL, TRUE, ARRAY['service_accounts.read']::TEXT[]),
    ('iam.sso', '/iam/sso', 'organization', 'all', NULL, NULL, TRUE, ARRAY['org.sso.read']::TEXT[]),
    ('iam.sso.test', '/iam/sso/tests/:id', 'organization', 'all', NULL, NULL, TRUE, ARRAY['org.sso.manage']::TEXT[]),
    ('iam.email.domains', '/iam/email-domains', 'organization', 'all', NULL, NULL, TRUE, ARRAY['org.email_domains.read']::TEXT[]),
    ('settings', '/settings', 'any', 'any', 'admin', 20, TRUE, ARRAY['org.preferences.read', 'org.sharing.read', 'org.cipher_keys.read', 'org.regex_patterns.read', 'org.field_masking.read', 'org.domains.read', 'org.running_queries.read', 'org.signin.read', 'audit.read', 'notify.connectors.read', 'notify.policies.read', 'notify.templates.read', 'notify.defaults.read', 'notify.deliveries.read', 'alerts.calendars.read', 'alerts.importance.read', 'sys.organizations.read', 'sys.administrators.read', 'sys.settings.retention.read', 'sys.settings.limits.read', 'sys.settings.signin.read', 'sys.settings.alerting.read', 'sys.settings.topology.read', 'sys.clusters.read', 'sys.nodes.read', 'sys.query.admission.read']::TEXT[]),
    ('settings.general', '/settings/general', 'organization', 'any', NULL, NULL, TRUE, ARRAY['org.preferences.read', 'org.sharing.read']::TEXT[]),
    ('settings.organization', '/settings/organization', 'organization', 'all', NULL, NULL, TRUE, ARRAY['org.preferences.read']::TEXT[]),
    ('settings.audit', '/settings/audit', 'organization', 'all', NULL, NULL, TRUE, ARRAY['audit.read']::TEXT[]),
    ('settings.notify', '/settings/notify/*', 'organization', 'any', NULL, NULL, TRUE, ARRAY['notify.connectors.read', 'notify.policies.read', 'notify.templates.read', 'notify.defaults.read', 'notify.deliveries.read', 'org.members.read']::TEXT[]),
    ('settings.alerting', '/settings/alerting/*', 'organization', 'any', NULL, NULL, TRUE, ARRAY['alerts.calendars.read', 'alerts.importance.read']::TEXT[]),
    ('settings.sso.providers', '/settings/sso_providers', 'organization', 'all', NULL, NULL, TRUE, ARRAY['org.sso.read']::TEXT[]),
    ('settings.tenant.tools', '/settings/:section', 'organization', 'any', NULL, NULL, TRUE, ARRAY['org.cipher_keys.read', 'org.regex_patterns.read', 'org.field_masking.read', 'org.domains.read', 'org.running_queries.read']::TEXT[]),

    -- Root-only system IAM and settings.
    ('iam.organizations', '/iam/organizations', 'system', 'all', NULL, NULL, TRUE, ARRAY['sys.organizations.read']::TEXT[]),
    ('settings.organization.management', '/settings/organization_management', 'system', 'all', NULL, NULL, TRUE, ARRAY['sys.organizations.read']::TEXT[]),
    ('settings.client_ip', '/settings/client_ip', 'system', 'all', NULL, NULL, TRUE, ARRAY['sys.settings.signin.read']::TEXT[]),
    ('settings.nodes', '/settings/nodes', 'system', 'any', NULL, NULL, TRUE, ARRAY['sys.clusters.read', 'sys.settings.topology.read']::TEXT[]),
    ('settings.signin_requirements', '/settings/signin_requirements', 'system', 'all', NULL, NULL, TRUE, ARRAY['sys.settings.signin.read']::TEXT[]),
    ('settings.signin_limits', '/settings/signin_limits', 'system', 'all', NULL, NULL, TRUE, ARRAY['sys.settings.signin.read']::TEXT[]),
    ('settings.audit_retention', '/settings/audit_retention', 'system', 'all', NULL, NULL, TRUE, ARRAY['sys.settings.retention.read']::TEXT[]),
    ('settings.stream_retention', '/settings/stream_retention', 'system', 'all', NULL, NULL, TRUE, ARRAY['sys.settings.retention.read']::TEXT[]),
    ('settings.series_limits', '/settings/series_limits', 'system', 'all', NULL, NULL, TRUE, ARRAY['sys.settings.limits.read']::TEXT[]),
    ('settings.intake_usage', '/settings/intake_usage', 'system', 'all', NULL, NULL, TRUE, ARRAY['sys.settings.limits.read']::TEXT[]),
    ('settings.alert_evaluation', '/settings/alert_evaluation', 'system', 'all', NULL, NULL, TRUE, ARRAY['sys.settings.alerting.read']::TEXT[]);

DELETE FROM iam_routes route
WHERE NOT EXISTS (
    SELECT 1 FROM iam_route_seed seed WHERE seed.route_key = route.route_key
);

INSERT INTO iam_routes (
    route_key,
    path_pattern,
    scope,
    permission_mode,
    navigation_group,
    navigation_position,
    enabled,
    catalog_version,
    permissions
)
SELECT
    route_key,
    path_pattern,
    scope,
    permission_mode,
    navigation_group,
    navigation_position,
    enabled,
    2,
    permissions
FROM iam_route_seed
ON CONFLICT (route_key) DO UPDATE
SET path_pattern = EXCLUDED.path_pattern,
    scope = EXCLUDED.scope,
    permission_mode = EXCLUDED.permission_mode,
    navigation_group = EXCLUDED.navigation_group,
    navigation_position = EXCLUDED.navigation_position,
    enabled = EXCLUDED.enabled,
    catalog_version = EXCLUDED.catalog_version,
    permissions = EXCLUDED.permissions;

CREATE OR REPLACE FUNCTION bump_iam_route_catalog_version()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE iam_catalog_versions
       SET version = version + 1,
           updated_at = clock_timestamp()
     WHERE catalog_key = 'routes';
    RETURN NULL;
END
$$;

DROP TRIGGER IF EXISTS trg_iam_routes_catalog_version ON iam_routes;
CREATE TRIGGER trg_iam_routes_catalog_version
AFTER INSERT OR UPDATE OR DELETE ON iam_routes
FOR EACH STATEMENT EXECUTE FUNCTION bump_iam_route_catalog_version();
