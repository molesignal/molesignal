/** The pt-br translations, one chunk loaded when the language is chosen. */
export default import.meta.glob<Record<string, unknown>>('../pt-br/*.json', { eager: true, import: 'default' });
