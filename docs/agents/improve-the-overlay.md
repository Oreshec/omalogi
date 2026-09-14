# Prompt: improve the overlay

```text
Read AGENTS.md, especially "Working on the overlay", first and follow its rules.

What I want: <the change, e.g. "show the DPI level next to each profile in the list">.

1. Look at how Omarchy's own plugins do similar things (under $OMARCHY_PATH/shell) and
   use the same qs.Commons and qs.Ui components, theme colours and spacing. Tell me which
   ones you will reuse.
2. Put any logic in plugin/Model.js with a Node test in plugin/tests/.
3. Keep the QML declarative; add new QML files to PLUGIN_FILES in src/setup.rs.
4. Run every check in AGENTS.md, including qmllint on the files you changed.
5. Tell me the commands to install and restart the shell, and what to look at. Only open
   the overlay or take a screenshot yourself if I say so, and crop it to the overlay.

Text in the UI names what people recognise and says exactly what happens.
```
