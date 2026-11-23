/** The zh-cn translations, one chunk loaded when the language is chosen. */
export default import.meta.glob<Record<string, unknown>>('../zh-cn/*.json', { eager: true, import: 'default' });
