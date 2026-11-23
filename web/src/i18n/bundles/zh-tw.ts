/** The zh-tw translations, one chunk loaded when the language is chosen. */
export default import.meta.glob<Record<string, unknown>>('../zh-tw/*.json', { eager: true, import: 'default' });
