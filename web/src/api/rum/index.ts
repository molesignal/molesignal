/**
 * RUM client. The explorer reads the rows the SDKs sent with `search`, `facets` and
 * `histogram`, and the numbers over them with `aggregate`, which a dashboard panel draws with the
 * statement of `panelStatement`; a session is read with `timeline`,
 * `replay` and `relatedTraces`; the applications that send them are managed in `applications`,
 * and the errors they send are grouped into the issues of `issues`.
 */
export * from './aggregate';
export * from './alerts';
export * from './analytics';
export * from './applications';
export * from './explorer';
export * from './heatmaps';
export * from './issues';
export * from './panel';
export * from './playlists';
export * from './session';
