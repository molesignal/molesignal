/** The de translations, one chunk loaded when the language is chosen. */
export default import.meta.glob<Record<string, unknown>>('../de/*.json', { eager: true, import: 'default' });
