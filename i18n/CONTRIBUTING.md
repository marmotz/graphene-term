# Translating Graphene

The texts of the interface live in the [Fluent](https://projectfluent.org) files of this
folder, one per language, embedded in the binary at build time.

| File | Language | Role |
|---|---|---|
| `en-US.ftl` | English (United States) | The reference: every message, with its comments |
| `en-GB.ftl` | English (United Kingdom) | Variant: only the texts that differ from `en-US` |
| `fr.ftl` | French | Base language |
| `fr-CA.ftl` | French (Canada) | Variant: only the texts that differ from `fr` |
| `es.ftl`, `de.ftl`, `it.ftl` | Spanish, German, Italian | Base languages |
| `languages.ftl` | (all) | The name of every language in itself, never translated |

A text missing from a file falls back to the parent language of a variant, then to
`en-US`. A base language must define every message of `en-US.ftl`: the tests fail
otherwise.

## Review status

The first line of every language file gives its status:

- `## Status: reviewed`: a person who speaks the language has read every text.
- `## Status: unreviewed`: the texts were translated by AI from `en-US.ftl` and
  `fr.ftl`, and nobody has read them yet.

## Review a language

1. Open the file of the language next to `en-US.ftl`, which documents each message
   (what it shows, what its `{ $variables }` hold).
2. Read every text and fix what sounds wrong, is too long for its place or does not
   follow the usual terms of the system (for example the name of the Settings window).
3. Try it: set `language = "<tag>"` in the configuration file (`es`, `de`, `it`...) and
   go through the menus, the command palette, the Settings tab and the dialogs.
4. When the whole file has been read, change its first line to `## Status: reviewed`.

## Fix a translation

- Change only the text after `=`, never the identifier before it.
- Keep every `{ $variable }` of the English text; their order may change.
- In a selector (`{ $count -> ... }`), keep every key in brackets and the `*` of the
  default one. The plural keys (`[one]`, `[other]`) follow the
  [CLDR rules](https://www.unicode.org/cldr/charts/latest/supplemental/language_plural_rules.html)
  of the language.
- Numbers are written by the application in the format of the language: do not add
  separators around `{ $variables }`.
- Paths, names and values in backticks or quotes are written as in the configuration
  file and stay untranslated.

## Check your change

```bash
cargo test i18n
```

The tests check that every file parses, that a base language defines every message,
that no file defines an unknown identifier and that each value of a selector has its
own text.

## Add a language

Adding a language also needs code in `src/i18n.rs` (`Lang`, `LANGS`, `tag`, `source`,
`language_id`) and in `src/config/values.rs` (the `language` setting). Open an
[issue](https://github.com/marmotz/graphene-term/issues) to ask for it.
