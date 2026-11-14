# Updating the Configuration at Startup — Technical Design

Product decisions: [overview.md](overview.md).

## Current State

- `Config::load` delegates to `load_from` ([src/config.rs:84](../../../src/config.rs)). On
  initial load, an existing file is passed through `config_edit::complete_defaults` and
  atomically replaced only if completion changes it ([src/config.rs:122-136](../../../src/config.rs)).
  A missing file is created from the completed embedded template
  ([src/config.rs:138-150](../../../src/config.rs)).
- The configuration template is embedded from
  [config.default.toml](../../../config.default.toml) ([src/config.rs:21](../../../src/config.rs)).
  It documents options as commented lines with their default values
  ([config.default.toml:1](../../../config.default.toml)); all options are currently
  commented out. `README.md` confirms that users uncomment a line to change it
  ([README.md:12](../../../README.md)).
- `Config::default` contains the effective default values
  ([src/config.rs:359](../../../src/config.rs)). `Config::parse` reads the TOML keys that
  are present, initializes the others with those defaults, ignores unknown keys while
  reporting an error, and leaves the previous configuration unchanged if the TOML syntax
  is invalid ([src/config.rs:408](../../../src/config.rs)). Section and key recognition
  is implemented in `Config::parse` and `Reader::read_section_key`
  ([src/config.rs:408](../../../src/config.rs), [src/config.rs:563](../../../src/config.rs)).
- Built-in shortcuts are defined separately in the `DEFAULTS` table
  ([src/keybindings.rs:95](../../../src/keybindings.rs)); each `Action` provides its
  configuration ID (`Action::id`, [src/input.rs:96](../../../src/input.rs)). The current
  template only documents examples for `[keybindings]`
  ([config.default.toml:59](../../../config.default.toml)).
- The `uncommented_default` test turns the commented template into active TOML to verify
  that it parses and matches `Config::default` ([src/config.rs:739-747](../../../src/config.rs)).
  This provides a basis for checking that the template remains consistent with the
  configuration schema.
- At graphical startup, `app::run` loads the configuration before starting the watcher
  ([src/app.rs:1946](../../../src/app.rs), [src/app.rs:1969](../../../src/app.rs)). The
  initial update can therefore complete before changes are watched. The watcher then
  reports changes to `config.toml` ([src/watcher.rs:83](../../../src/watcher.rs)).
- The binary currently handles only `--version`; all other arguments launch the graphical
  application ([src/main.rs:1](../../../src/main.rs)).
- `toml_edit = "0.25.19"` is a direct dependency ([Cargo.toml:36](../../../Cargo.toml)); it
  preserves source spans for pruning and validates TOML before startup edits.

## Technical Decisions

### Source of Truth and Startup Update

- The supported schema is the one recognized by the `Config` parser. `DEFAULT_CONFIG`
  remains the source for documented lines and static values; `keybindings::DEFAULTS`
  provides the complete list of built-in shortcuts, and `Action::id` provides their
  configuration values. The test that activates the template continues to verify that it
  matches `Config::default`.
- During initialization (`load_from`), compare the supported options in the template
  with the existing file. An option that is already present, either active **or
  commented**, must not be added again. Add absent options as commented lines under their
  existing section, before the next section starts. Reuse either an active section header
  or an existing commented template header; do not append a second header for a section
  already present. Add one commented header only when the file has no corresponding
  section. For `[keybindings]`, include every built-in shortcut, not just the examples
  currently in `config.default.toml`; each line shows the corresponding action
  (`Action::id`).
- Expose the built-in key/action pairs through a `pub(crate)` iterator in
  `keybindings.rs`, derived from `DEFAULTS`, so configuration lines can be generated
  without maintaining a second hard-coded list. Existing shortcuts, whether active or
  commented, are neither duplicated nor replaced.
- Root-level options are inserted in the root section, before the first table. Preserve
  all existing text, user comments, active values, unknown keys, ordering, and formatting
  except for a malformed commented setting that can be unambiguously matched to a
  supported option; normalize that line to the corresponding template comment rather than
  adding a duplicate.
- Do not add a schema version: each startup can fill in missing options. This avoids
  maintaining a migration table for a file whose missing options already use the values
  from `Config::default`.
- Update the file only during initial loading, not in `reload_from`: reloading after an
  external edit must not write to the file.
- If TOML syntax cannot be parsed, do not rewrite the file and retain the current error
  banner behavior. Invalid values in syntactically valid TOML do not prevent adding
  missing options. If writing fails, log the error and continue with the existing
  configuration-loading behavior.

Alternatives:

- *Regenerate the whole file from the template*: rejected for this implementation because
  reliable in-place insertion is feasible and avoids losing user comments, key order, and
  formatting. Keep regeneration as a fallback only if future schema changes make safe
  in-place completion impractical; such a fallback must carry forward active supported
  values and unknown entries.
- *Add active TOML lines*: rejected because the current configuration file documents
  options as commented examples that users can uncomment to customize.
- **Insert only absent options as commented lines in their existing sections**: retained
  because it follows the current template, avoids duplicate section headers, and does not
  replace user-configured values. Generate the full built-in shortcut list from
  `DEFAULTS`.

### Editing and Preserving Text

- `src/config_edit.rs` owns template analysis, detection of existing options, insertion of
  commented lines, and pruning. `complete_defaults` checks active TOML nodes plus
  commented options, then inserts additions in existing section blocks or creates a
  single commented header for an absent section
  ([src/config_edit.rs:23-110](../../../src/config_edit.rs)). The previous implementation
  appended every missing option at EOF, which caused the repeated section headers
  observed in existing files.
- Use the direct `toml_edit = "0.25.19"` dependency and its syntax tree and spans to
  recognize active keys and remove only the targeted TOML entries without serializing
  the entire document. For commented template options and section headers, scan the
  original text while tracking both active and commented table headers (`section_blocks`
  and `commented_options`, [src/config_edit.rs:156-180](../../../src/config_edit.rs),
  [src/config_edit.rs:569-599](../../../src/config_edit.rs)). Parse quoted keys using TOML
  syntax where possible; tolerate a leading stray quote on a commented known key so
  malformed examples such as `# "shape = "block"` are normalized in place rather than
  duplicated ([src/config_edit.rs:195-235](../../../src/config_edit.rs)).
- Group missing options by section and apply insertions from the end of the file toward
  the beginning so earlier byte offsets remain valid. Place each insertion at the end of
  the existing active or commented section block. For an absent section, append one
  commented section header and its missing options. For root-level keys, insert before
  the first table header. The edit must preserve all unaffected source slices exactly.
- Return unknown keys as structured data (path and span) from configuration parsing
  instead of deriving them from `ConfigError` messages. Reuse the same recognition
  branches as `Config::parse` so that parsing and pruning agree on which keys are valid.
- Validate changes in memory before touching the file. Write to a temporary file in the
  same directory, then atomically rename it over `config.toml`; never truncate the only
  copy before the replacement is ready.
- Never modify a document with invalid TOML syntax, whether the request comes from startup
  or `--prune-config`.

### Explicit Cleanup

- Handle `--prune-config` in `src/main.rs` before calling `graphene::run`, so the command
  does not create a window or event loop. Delegate path resolution, default-file
  creation, detection, and editing to `config_edit`/`config`; expose only the required
  entry point from `src/lib.rs`.
- If the file does not exist, create it from `DEFAULT_CONFIG`, apply the same completion
  of missing options as at startup, then report that there is nothing to prune.
- Remove only keys the parser does not recognize. A recognized key with an invalid value
  is not an unknown key and must remain. In a recognized section, remove only unknown
  entries; remove an entire unknown section. List the target key paths before making any
  changes.
- In an interactive terminal, ask for confirmation before writing. `--yes` confirms
  explicitly and allows non-interactive use; without a terminal or `--yes`, exit without
  modifying the file. If there is nothing to remove, report that without asking for
  confirmation.
- A read error, invalid TOML syntax, or write error must produce an explicit failure and
  leave the original file intact. Do not use formatted `ConfigError` messages as a
  classification interface; provide structured unknown-key paths to the pruning
  routine.

Alternatives:

- *Remove unknown keys automatically at startup*: rejected because this could remove an
  option belonging to a newer version or an extension.
- *Classify keys by parsing error-message text*: rejected because message formatting is
  for display and is not a stable data interface.
- **Explicit command with a list, confirmation, and `--yes`**: retained because cleanup
  is intentional while still being scriptable through an explicit flag.

### Tests and Documentation

- Add unit tests to `config_edit` for active and commented options being present or
  absent, insertion into existing active and commented sections, no duplicate section
  headers, root-level insertion, normalization of a malformed commented known option,
  duplicate avoidance, exact preservation of unaffected text, custom values and unknown
  keys, no write on invalid TOML, and no write when nothing is missing. Also verify that
  each entry in `keybindings::DEFAULTS` is exposed as a commented line with the correct
  `Action::id` when absent.
- Test pruning of unknown root-level and nested keys, preservation of recognized keys
  with invalid values, no write on syntax errors, confirmation/refusal/`--yes` paths, and
  creation of the default file when it is missing.
- Extend the existing missing-file test in `src/config.rs` and add argument-dispatch
  tests to verify that `--prune-config` does not launch the graphical application.
- Update the README configuration section to explain automatic completion of missing
  options inside existing sections, and document `graphene --prune-config` and `--yes`.

## Verified Consequences

- Existing files retain their settings and text content; only supported options absent
  from the file are inserted, in their matching section. User-configured options must
  never be replaced by defaults, and a section must not gain a duplicate header.
- Unknown entries continue to be reported by the parser and preserved at startup; only
  explicit cleanup may remove them.
- `Config::parse` remains read-only. The startup migration is integrated into
  `load_from`, and CLI processing bypasses `app::run` entirely.
- Tests must cover any addition to the static schema or `keybindings::DEFAULTS`, ensuring
  that new options and shortcuts appear in existing configuration files.

## Découpage en tâches d'implémentation

1. [#93](https://github.com/marmotz/graphene-term/issues/93) Automatically complete
   missing configuration options at startup.
2. [#94](https://github.com/marmotz/graphene-term/issues/94) Add the explicit
   `--prune-config` cleanup command (depends on #93).
