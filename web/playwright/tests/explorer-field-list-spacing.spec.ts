import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults, type Explorer } from '../fixtures/mockExplorers';

/** A field of the list or a group of them: the cards a part of the list holds. */
const CARD = ':scope > div > [data-field-row], :scope > div > [data-field-group]';

test.describe('The parts of an explorer field list', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
  });

  for (const explorer of ['logs', 'traces'] as Explorer[]) {
    // The heading and its count set a part off. The air above it was three times what
    // parts cards from one another, and read as a hole in the list.
    test(`follow one another closely on ${explorer}`, async ({ page }) => {
      await page.goto(`/${explorer}`);
      await expectExplorerResults(page, explorer);

      const parts = await page.locator('[data-side-panel-section]').evaluateAll((sections, card) =>
        sections.map((section) => {
          const cards = Array.from(section.querySelectorAll(card));
          const heading = section.querySelector('button')!.getBoundingClientRect();

          return {
            headingTop: heading.top,
            headingHeight: heading.height,
            firstCardTop: cards[0]?.getBoundingClientRect().top ?? null,
            lastCardBottom: cards.at(-1)?.getBoundingClientRect().bottom ?? null,
            // What parts one card from the next inside the part.
            cardGap: cards.length > 1 ? cards[1]!.getBoundingClientRect().top - cards[0]!.getBoundingClientRect().bottom : null,
          };
        }), CARD);

      expect(parts.length).toBeGreaterThanOrEqual(2);
      for (const [index, part] of parts.entries()) {
        const before = parts[index - 1];

        expect(part.headingHeight).toBeLessThanOrEqual(28);
        // Its first card stands right under the heading.
        if (part.firstCardTop !== null) {
          expect(part.firstCardTop - (part.headingTop + part.headingHeight)).toBeLessThanOrEqual(4);
        }
        // And the heading no further under the last card of the part before than a card and a half.
        if (before?.lastCardBottom != null) {
          expect(part.headingTop - before.lastCardBottom).toBeLessThanOrEqual(8);
          expect(part.headingTop - before.lastCardBottom).toBeGreaterThanOrEqual(before.cardGap ?? 0);
        }
      }
    });
  }
});
