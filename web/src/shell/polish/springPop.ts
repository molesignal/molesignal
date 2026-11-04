/**
 * A toggle that marks something as the reader's own (a favourite, a like, a pin) answers the
 * click that turns it on with a small spring: its icon starts small, overshoots and settles
 * (`[data-ms-pop]`, polish/motion.css). Turning it off has no spring, the colour just leaves.
 */
export function springPop(element: HTMLElement): void {
  element.removeAttribute('data-ms-pop');
  // Reading the layout restarts the spring when the toggle is clicked again quickly.
  void element.offsetWidth;
  element.setAttribute('data-ms-pop', '');
  element.addEventListener('animationend', () => element.removeAttribute('data-ms-pop'), {
    once: true,
  });
}
