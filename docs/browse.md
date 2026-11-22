# Browse

`casemap browse` and `casemap map` open the activity map in an interactive
terminal. Pick a use case and press Enter to see its diagram. Bare `casemap`
opens the same view when the repository already has a catalog.

In a diagram, selecting a step highlights the paths leading into it. The
panel shows its anchors, a code preview when available and steps that return
to it. Press Enter to open the selected step's anchored code.

With `--base REF`, browse compares against the merge base of REF and HEAD.
The default is HEAD. Press `d` for diff marks or `v` to see both versions.

| Key | Activity map | Diagram |
|---|---|---|
| Arrow keys | Select a flow | Select a step |
| Enter | Open the selected flow | Open anchored code |
| Tab | Open the selected flow | Return to the map |
| `h` `j` `k` `l` | `j` and `k` select flows | Pan one row or column |
| `H` `J` `K` `L` | | Pan ten rows or columns |
| PgUp and PgDn | Select by page | Pan by page |
| Home | First flow | Reset pan |
| `+` and `-` | | Zoom in or out |
| `/` | Search title or name | Search title or name |
| `d` or `v` | Set the comparison view | Toggle marks or side by side |
| `p` | Toggle the panel | Toggle the panel |
| `r` | Reload | Reload |
| `?` | Help | Help |
| Esc | Close help or search | Return to the map |
| `q` or Ctrl-C | Quit | Quit |

Changed flow files reload automatically. Press `r` to refresh anchored code
after editing it outside the catalog. If terminal graphics are unavailable,
the diagram panel shows the path to a temporary PNG.

`casemap browse` needs a terminal and the `tui` feature. `casemap map` with
redirected output still prints the grouped catalog for scripts. Use `--flows`
to select a catalog or `--theme` and `--bg` to style the diagram.
