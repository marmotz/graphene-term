# Graphene Term

## Documentation

Every change that adds, removes or changes a user-visible feature, shortcut, setting or
command-line option MUST update, in the same change:

- `docs/en/` and `docs/fr/`: the page that covers it, in both languages, and the index
  (`docs/en/README.md`, `docs/fr/README.md`) when a page or a topic is added.
- `README.md` and `README.fr.md`: the **Features** list, and the **Comparison** table
  when the feature belongs there (verify the other terminals' support before filling
  their columns, never guess).
- `docs/dev/` (architecture, decisions) when the change affects modules, threads, data
  flow or a technical choice.
- `config.default.toml` and the configuration pages when a configuration key changes.

English and French stay in sync: same content, same structure. Before considering a
change done, check that no page still describes the old behaviour (old shortcut, old
default, old option name).
