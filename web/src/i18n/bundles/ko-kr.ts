/** The ko-kr translations, one chunk loaded when the language is chosen. */
export default import.meta.glob<Record<string, unknown>>('../ko-kr/*.json', { eager: true, import: 'default' });
