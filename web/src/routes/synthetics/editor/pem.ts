/** The most a certificate chain or a private key can take, as the server holds it. */
export const MAX_PEM_BYTES = 64 * 1024;

/** The files a certificate or a key comes in. */
export const PEM_FILES = '.pem,.crt,.cer,.key';

const BLOCK = /-----BEGIN ([A-Z0-9 ]+)-----[\s\S]*?-----END \1-----/g;

/** The PEM in a text: its certificates, in order, and its first private key. */
export function pemParts(text: string): { certificates: string; key: string } {
  const blocks = text.match(BLOCK) ?? [];

  return {
    certificates: blocks.filter((block) => block.startsWith('-----BEGIN CERTIFICATE-----')).join('\n'),
    key: blocks.find((block) => /^-----BEGIN [A-Z ]*PRIVATE KEY-----/.test(block)) ?? '',
  };
}
