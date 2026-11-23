/** The ja-jp translations, one chunk loaded when the language is chosen. */
export default import.meta.glob<Record<string, unknown>>('../ja-jp/*.json', { eager: true, import: 'default' });
