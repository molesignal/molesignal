const STREAM_NAME = /^[A-Za-z0-9_.-]{1,255}$/;
const STREAM_PREFIX = /^[A-Za-z0-9_.-]{0,254}$/;
/** The product's own stream; no pipeline may read from or write to it. */
const SYSTEM_STREAM = '_molesignal';

/**
 * Whether a pipeline may name a stream this: the backend's rule for stream
 * names (`validate_stream_name`), less the product's own stream.
 */
export function isStreamName(name: string): boolean {
  return STREAM_NAME.test(name) && name !== '.' && name !== '..' && name !== SYSTEM_STREAM;
}

/** What a routing step may put in front of a stream's name: the same characters, or nothing. */
export function isStreamPrefix(prefix: string): boolean {
  return STREAM_PREFIX.test(prefix);
}
