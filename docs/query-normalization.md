# Query normalization

This document records the compatibility decisions behind query normalization in
`chinese_dictionary` 2.1.8. Keep it in sync with the normalizer and its tests when
changing dictionary generation or language detection upstream.

## Why normalization lives here

The embedded Chinese indexes already tolerate punctuation because Chinese
tokenization skips unindexed characters. English and Pinyin lookup use spaces as
token boundaries, and `chinese_detection` scores the characters it receives.
Passing raw punctuation to those paths can therefore change classification,
prevent a token match, or search only the tokens before the punctuation.

Normalization happens at the public library boundary so direct typed lookups and
automatically classified queries behave consistently for all consumers.

## Separator policy

Normalization trims leading and trailing separators and collapses runs of
Unicode whitespace or sentence punctuation to one ASCII space. The explicit
separator set covers common ASCII sentence delimiters, quotes, brackets,
slashes, dashes, ellipses, and their CJK, vertical, small-form, and full-width
equivalents.

The implementation intentionally does not classify every punctuation or symbol
code point as a separator. The English index contains meaningful symbol-bearing
keys, including `the lgbt+ community`, `less than <`, and `equals sign =`.
Operators and symbols such as `+`, `<`, `=`, `>`, `^`, `|`, and `~` must remain
available to those existing lookups.

All Unicode letters and digits pass through unchanged. This preserves Pinyin
tone marks, literal `ü`, and tone numbers.

## Pinyin compatibility exceptions

The Pinyin index has two representations that need special handling:

- It contains no straight or curly apostrophe-bearing keys. The entry 西安 is
  reachable through `xian`, `xi1an1`, and `xīān`, but not through `xi'an` or
  `xi’an`. An apostrophe surrounded by letters or digits is therefore treated as
  a non-separating joiner and removed. Boundary apostrophes remain quotation
  punctuation.
- The index deliberately uses ASCII `u:` for an umlaut in keys such as `lu:4`.
  General normalization treats a colon as a sentence separator so classification
  sees `lu 4`, while Pinyin lookup mode retains a colon immediately following
  `u` or `U` and searches the existing `lu:4` key.

These exceptions let the library use the current embedded data without merging
words across ordinary punctuation or regressing supported symbol queries.

## Upstream migration checklist

When correcting these representations in `syng-dictionary-creator` or
`chinese_detection`:

1. Decide on canonical keys and aliases for straight apostrophes, curly
   apostrophes, tone marks, tone numbers, literal `ü`, and `u:`.
2. Regenerate the Pinyin index and, if necessary, the detection profiles.
3. Verify exact ordered word-ID equivalence for all canonical and alias forms,
   especially 西安.
4. Keep the library canonicalization until regenerated data and detection both
   support the public query forms; changing only the index can still leave an
   automatically classified query routed to English.
5. If an exception can be removed, update this note and retain a regression test
   proving that clean and punctuated queries still have identical ordered IDs.

