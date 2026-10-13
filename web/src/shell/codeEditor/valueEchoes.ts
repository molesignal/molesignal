/**
 * Tells the text an editor's owner sends back from text the owner has set.
 *
 * An editor tells its owner every change, and the owner hands the text back as
 * its value. The two are not in step: a page that takes a moment to render is
 * still handing back the text of two keystrokes ago while the editor already
 * holds the next. Taken as the owner's word, that stale text would be written
 * over what was typed since, and the caret would jump to the start of it.
 *
 * So the editor remembers what it has told and not yet heard back. A value
 * that is one of those is the owner catching up, and changes nothing. Any
 * other value is the owner's own, and becomes the text.
 */
export interface ValueEchoes {
  /** The editor told its owner that its text is now `value`. */
  told: (value: string) => void;
  /**
   * Whether `value`, arriving from the owner while the editor holds `held`,
   * is the owner catching up with the editor rather than a text of its own.
   */
  isEcho: (value: string, held: string) => boolean;
}

/** As many values as are kept waiting to be heard back: an owner that never answers is not waited for. */
const WAITING_LIMIT = 64;

export function createValueEchoes(): ValueEchoes {
  let waiting: string[] = [];

  return {
    told(value) {
      waiting.push(value);
      if (waiting.length > WAITING_LIMIT) waiting = waiting.slice(-WAITING_LIMIT);
    },
    isEcho(value, held) {
      // The owner has caught up: nothing is waited for any more.
      if (value === held) {
        waiting = [];
        return true;
      }

      const heard = waiting.indexOf(value);

      // Never told: the owner has set a text of its own, and what was told before it is void.
      if (heard < 0) {
        waiting = [];
        return false;
      }

      // What was told after it is still on its way back.
      waiting = waiting.slice(heard + 1);
      return true;
    },
  };
}
