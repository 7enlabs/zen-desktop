# Bundled fonts

Source: Google Fonts, SIL Open Font License 1.1.

- Be Vietnam Pro, weights 400/600/700, subsets latin + vietnamese
  (static instances, one file per weight/subset).
- Noto Sans, weights 400/600/700, subsets latin + vietnamese.
  Google Fonts serves Noto Sans as a variable font, so the file for a
  given subset is the same across weights; the declared weight in
  styles.css selects the instance. Files are kept per weight to mirror
  the Google Fonts stylesheet exactly.

Referenced from src/styles.css with relative urls, so Tauri bundles
them with frontendDist (src/). No network fetch at runtime.
License texts: OFL-BeVietnamPro.txt and OFL-NotoSans.txt are the verbatim SIL Open Font License 1.1 texts distributed upstream with each family.
