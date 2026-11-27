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

## User interface

Every change to a window, panel or dialog MUST look polished, not just work:

- Give each control a clear place: a switch or checkbox carries its label next to it, and
  a setting that governs others sits above them, apart from the grid.
- Keep explanations short and secondary (weak or small text, close to what they explain),
  never a long paragraph pushed above the controls.
- Align, space and group related items consistently with the rest of the window; reuse
  the existing helpers and styles before adding new ones.
- Check the result as a user would see it (render it, or cover the layout with a test on
  the drawn shapes) before considering the change done.
