---
name: review-sql-migrations
description: Review MoleSignal PostgreSQL migrations and runtime sqlx queries for deployment safety, explicit embedded-migrator registration, tenant isolation, locking, indexes, conflict handling, data compatibility, and bounded results. Use for changes under src/infra/migrations, src/infra/persistence, repository SQL, schema evolution, indexes, backfills, or migration failures.
---

# 审查 SQL Migration 与 sqlx 查询

## 当前迁移机制

- migration 文件位于 `src/infra/migrations/*.sql`。
- 文件名格式为 `YYYYMMDDHHMMSS_<short_name>.sql`，版本必须唯一且递增。
- 运行时不会扫描目录，也不使用 `sqlx::migrate!`。
- 每个新文件都必须手工加入 `src/infra/persistence/pool.rs::embedded_migrator()` 的 `include_str!` 列表。
- `embedded_migrations_match_files_on_disk` 单测要求磁盘文件与注册列表完全一致。
- 已发布 migration 视为不可变；通过追加新文件演进 schema。
- `-- no-transaction` 必须位于文件开头，才会被当前注册 helper 识别为非事务 migration。

ID 通常是 string-backed `Id`，数据库列使用 `TEXT` 或 `VARCHAR(64)`；时间字段使用微秒 `BIGINT`。

## Migration 检查

1. 文件版本是否与现有文件冲突，是否同步注册到 `embedded_migrator()`。
2. schema 是否保持多租户列、非空约束和以组织列开头的常用索引。
3. 是否直接删除/重命名列或改变类型；线上兼容变更优先采用新增、双写、回填、切换、后续删除。
4. 是否对热表执行无界 UPDATE、全表 rewrite、长事务或高锁级 DDL。
5. `CREATE INDEX CONCURRENTLY` 是否配合非事务 marker，并明确失败后的恢复策略。
6. 新约束是否会被历史数据违反；是否需要先校验或分阶段收紧。
7. seed/upsert 是否幂等，重复部署是否安全。
8. 外键删除策略和唯一约束是否符合资源生命周期。

## sqlx 查询检查

1. 所有租户数据的读写是否包含 `org_id` 或 `organization_id` 谓词。
2. `ON CONFLICT` 列是否匹配实际 UNIQUE/PRIMARY KEY。
3. 列表查询是否有边界、分页和稳定排序。
4. 动态值是否使用参数绑定；标识符白名单化，不拼接不可信输入。
5. PostgreSQL `BIGINT` 与 Rust `i64` 是否一致；转 `u64` 前是否校验。
6. JSONB 是否可向前/向后兼容反序列化。
7. 并发领取任务是否正确使用 advisory lock、`FOR UPDATE` 或 `SKIP LOCKED`。
8. 错误是否通过 `src/infra/persistence/mod.rs::sqlx_err` 或等价的显式映射处理 `RowNotFound`、`23505` 和其他 DB 错误。
9. 不要引入依赖在线 `DATABASE_URL` 的 `query_as!`；本项目使用 runtime query API 和本地 `sqlx-shim`。

## 输出

1. 是否可安全部署
2. migration 注册或兼容性问题
3. 锁表/回填风险
4. 缺少组织隔离、索引或错误 conflict target 的查询
5. 推荐的分阶段 migration 或参数化 patch
