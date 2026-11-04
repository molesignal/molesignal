# Product faces

The interface is set in faces served with the app and declared in
`../tokens.css`. Nothing is loaded from a font CDN at runtime.

The product face, `MoleSignal Sans`, picks its file by weight:

- 400, text, data and menus: Alibaba Sans Regular for Latin, Greek and
  Cyrillic (W1G); Chinese falls to Alibaba PuHuiTi 3.0 Regular, the next
  family in `--font-sans`.
- 500, buttons, labels and table heads: Alibaba PuHuiTi 3.0 Regular, Latin
  (W1G) and Chinese alike.
- 600 to 700, titles, as Ant Design Pro sets them: Alibaba Sans Bold for
  Latin; Chinese falls to Alibaba PuHuiTi 3.0 SemiBold.

## Alibaba Sans

`alibaba-sans-regular.woff2` and `alibaba-sans-bold.woff2` are Alibaba Sans
1.02 Regular and Bold as published on https://www.alibabafonts.com/,
unmodified (Latin W1G, tabular figures by default). The family has no
SemiBold, so the titles' 600 is its Bold.

## Alibaba PuHuiTi 3.0

`alibaba-puhuiti-3-regular.woff2` and `alibaba-puhuiti-3-semibold.woff2` are
subsets of Alibaba PuHuiTi 3.0 (version 3.01) 55 Regular and 75 SemiBold from
the same site, the Chinese companion of Alibaba Sans. Each holds:

- the 3,755 characters of GB2312 level 1, the common Chinese characters;
- every character above U+2000 in `web/src`, `web/public` and
  `web/index.html` when the subset was made;
- Basic Latin, Latin-1, general punctuation, currency, letterlike, arrows,
  mathematical operators, enclosed alphanumerics, geometric shapes,
  miscellaneous symbols, CJK punctuation, vertical forms and full-width forms;
- Regular only: the rest of Latin W1G (Latin Extended-A and -B, spacing
  modifiers, combining marks, Latin Extended Additional), Greek and Cyrillic,
  since buttons, labels and table heads take their Latin from it.

A character outside a subset falls through to the platform's Chinese face
(`--font-sans`). To make the subsets again, collect those characters into a
file and run, for each weight:

```
pyftsubset AlibabaPuHuiTi-3-55-Regular.ttf --text-file=chars.txt \
  --flavor=woff2 --no-hinting --layout-features='*' --name-IDs='*' \
  --output-file=alibaba-puhuiti-3-regular.woff2
```

Alibaba Sans and Alibaba PuHuiTi are free for commercial and personal use;
the terms are on https://www.alibabafonts.com/.

## JetBrains Mono: code

`jetbrains-mono-latin-wght-normal.woff2` is the product's monospace face
(`--font-mono` and `--font-code`): the code editors, query syntax, JSON and
log lines read on their own.
Chrome cannot reach the platform's SF Mono, so the face is served with the
app.

- One variable file, weights 100–800, upright only.
- Latin subset (Basic Latin, Latin-1, general punctuation and a few symbols);
  other scripts fall through the rest of the `--font-mono` stack.
- Its metrics match Menlo's (advance 0.6 em, x-height 0.55), so a column is as
  wide whichever of the two draws it.

Source: JetBrains Mono as packaged by `@fontsource-variable/jetbrains-mono`
5.3.0 (`files/jetbrains-mono-latin-wght-normal.woff2`). Licensed under the SIL
Open Font License 1.1: `JetBrainsMono-OFL.txt`.

- https://www.jetbrains.com/lp/mono/
- https://github.com/JetBrains/JetBrainsMono
