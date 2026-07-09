# Color Themes

Hubuum CLI colors are role based. A theme maps roles such as `heading`,
`command`, `prompt`, and `table_band` to terminal colors.

Bundled public palettes are limited to MIT licensed, or clearly MIT-compatible,
sources. The current bundled external palettes are:

- Catppuccin Mocha and Latte, from Catppuccin under the
  [MIT license](https://github.com/catppuccin/catppuccin/blob/main/LICENSE).
- Solarized Dark and Light, from Solarized under the
  [MIT license](https://github.com/altercation/solarized/blob/master/LICENSE).

First-party `hubuum-dark` and `hubuum-light` themes are distributed under this
project's MIT license.

## Selecting Themes

List available themes:

```console
hubuum-cli theme list
```

Preview a theme without changing config:

```console
hubuum-cli theme preview catppuccin-mocha
```

Persist a theme and reload the current REPL session:

```console
theme use solarized-dark
```

You can also select themes with config, environment, or startup flags:

```console
hubuum-cli --theme hubuum-dark
HUBUUM_CLI__OUTPUT__THEME=catppuccin-mocha hubuum-cli
config set --key output.theme --value solarized-dark
```

`--color never` still disables all ANSI styling regardless of the selected
theme.

File redirects use the same color mode. `auto` treats files as non-terminals
and strips ANSI styling, `never` also strips it, and `always` preserves styling
codes in the file.

## Custom Theme Files

Set `output.theme_file` or pass `--theme-file` to load additional local themes.
Custom theme files are TOML and may inherit from a built-in or another custom
theme.

```toml
[[theme]]
name = "night-ops"
display_name = "Night Ops"
inherits = "hubuum-dark"

[theme.roles]
command = { fg = "#7ee787" }
heading = { fg = "ansi:cyan", bold = true }
table_band = { bg = "ansi256:235" }
```

Valid color forms are:

- `#rrggbb`
- `ansi:<name>`, such as `ansi:green` or `ansi:bright-cyan`
- `ansi256:<0-255>`

Custom theme names must use lowercase letters, numbers, and dashes.
