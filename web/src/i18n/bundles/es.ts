/** The es translations, one chunk loaded when the language is chosen. */
export default import.meta.glob<Record<string, unknown>>('../es/*.json', { eager: true, import: 'default' });
