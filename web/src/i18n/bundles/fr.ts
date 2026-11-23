/** The fr translations, one chunk loaded when the language is chosen. */
export default import.meta.glob<Record<string, unknown>>('../fr/*.json', { eager: true, import: 'default' });
