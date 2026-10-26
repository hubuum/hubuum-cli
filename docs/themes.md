# Color Themes

Hubuum CLI colors are role based. A theme maps structural roles such as `key`,
`table_header`, `title`, `border`, and `message`, and classified field values,
to terminal colors. Classification happens during text rendering, after pipeline
transforms; JSON, JSONL, CSV, and TSV retain their original data without styling.

Bundled public palettes are limited to MIT licensed, or clearly MIT-compatible,
sources. The current bundled external palettes are:

- Catppuccin Mocha and Latte, from Catppuccin under the
  [MIT license](https://github.com/catppuccin/catppuccin/blob/main/LICENSE).
- Solarized Dark and Light, from Solarized under the
  [MIT license](https://github.com/altercation/solarized/blob/master/LICENSE).

First-party themes are distributed under this project's MIT license.

## Bundled Themes

Themes intended for dark terminal backgrounds:

| Name | Character |
| --- | --- |
| `hubuum-dark` | Restrained ANSI defaults |
| `catppuccin-mocha` | Soft pastels |
| `solarized-dark` | Classic low-contrast Solarized |
| `aurora-night` | Icy blue, teal, and aurora green |
| `synthwave-sunset` | Neon cyan, magenta, and violet |
| `ember-forge` | Copper, amber, and cooling teal |
| `phosphor-green` | Green-screen terminal glow |
| `signal-high-contrast` | Bright, highly separated status colors |
| `rose-pink` | Layered rose and hot-pink tones |
| `ocean-blue` | Deep navy through bright sky blue |
| `royal-purple` | Aubergine, violet, and pale lavender |
| `emerald-green` | Forest shadows and vivid emerald |
| `lagoon-cyan` | Deep teal through luminous cyan |

Themes intended for light terminal backgrounds:

| Name | Character |
| --- | --- |
| `hubuum-light` | Restrained light defaults |
| `catppuccin-latte` | Soft light pastels |
| `solarized-light` | Classic Solarized light |
| `arctic-day` | Crisp blue and teal |
| `inkstone-light` | Neutral editorial ink tones |

The single-color families keep errors red and warnings yellow. Their remaining
roles progress from dark table bands through muted mid-tones to saturated
commands and lighter bold headings, so the chosen color remains recognizable
without obscuring semantic status.

## Selecting Themes

List available themes:

```console
hubuum-cli theme list
```

Preview a theme without changing config:

```console
hubuum-cli theme preview catppuccin-mocha
```

The preview includes structural roles, sample value classifications, and a dense
table with alternating row bands. The selected
theme's `table_band` role controls the shaded rows, independently of the active
theme and the configured table style. Normal command output applies that role
only to dense tables when `output.table_bands` is `auto` or `always`.

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

## Structural Roles

The renderer assigns these roles based on the element's purpose, independently
of its text. A header named `true` remains a header; a message containing `42`
remains a message.

| Role | Used for | Default |
| --- | --- | --- |
| `key` | Detail field names, including nested keys | `heading` |
| `table_header` | Table column headers | `heading` |
| `title` | Help titles, section headings, preview titles | `heading` |
| `border` | Table borders and dense-table separators | `muted` |
| `message` | Semantic confirmation and empty-result messages | Normal text |
| `table_band` | Alternating dense-table row backgrounds | Palette background |
| `error`, `warning` | Error and warning messages | Palette status colors |
| `prompt` | Interactive prompt | Palette prompt color |
| `heading` | Shared default for keys, headers, and titles | Palette heading color |
| `muted` | Explicit muted presentation and null-like defaults | Palette muted color |

Existing theme files continue to work. Defaults follow overrides: changing
`heading` updates keys, table headers, and titles unless those roles have explicit
overrides. `theme show NAME` lists the effective style for every role and the
classifier settings.

## Value Classifiers

There are twelve categories. Classification uses the entire trimmed field value;
it does not highlight matching substrings. Boolean and null-like matching ignores
ASCII case. Classification does not rewrite the value or change its semantic type.

| Category / role | Recognizes | Default style |
| --- | --- | --- |
| `number` | Signed integers, decimals, scientific notation, e.g. `-42`, `.5`, `6.02e23` | Prompt foreground, without bold |
| `ipv4` | Valid IPv4 addresses, e.g. `192.168.0.1` | `number` |
| `ipv6` | Valid IPv6 addresses, e.g. `2001:db8::1` | `number` |
| `boolean_true` | `true` | `number` |
| `boolean_false` | `false` | `number` |
| `null_like` | `null`, `none`, `nil`, `n/a` | `muted` |
| `command` | Configured command prefix followed by whitespace or end of text | Palette command/success color |
| `cidr` | IPv4/prefix `0–32` or IPv6/prefix `0–128`, e.g. `192.168.0.1/24`, `2001:db8::/32` | `number` |
| `date` | Valid `YYYY-MM-DD` dates and RFC3339 timestamps with a timezone | `number` |
| `url` | Absolute URLs with `://` and a host, plus `file:///` URLs | `command` |
| `mac` | Six hexadecimal octets with colons/dashes, or three dotted groups of four digits | `number` |
| Ordinary / `value` | Everything else, including empty text | Normal text |

Recognition order is booleans, null-like, numbers, IP addresses, CIDRs, dates,
URLs, MAC addresses, then commands. The first match wins. CIDRs may include host
bits. MAC examples are `00:1a:2b:3c:4d:5e`, `00-1a-2b-3c-4d-5e`, and
`001a.2b3c.4d5e`. A twelve-digit string is a number, not a MAC address.

There are no classifiers for hostnames or units such as `10 MB`. Ambiguous dates
such as `01/02/2026`, local timestamps without a timezone, bare URLs without a
scheme, `mailto:` addresses, and malformed values remain ordinary text. Numbers
use decimal syntax, excluding `NaN`, infinity, hexadecimal, and digit separators.
Existing blank rendering of missing/null table and top-level detail cells is
preserved; a visible `null` is classified as null-like.

`muted` is a presentation role applied after classification, not a value
category. Each category can have its own style, including separate true/false
styles. An empty style such as `ipv6 = {}` selects normal text for that category.
Dense-table bands combine with value styles; an explicit value background takes
precedence over the band background. Wrapping retains the original field's
classification on every displayed line.

## Tuning Classification and Styles

Configure classifiers alongside role overrides in a custom theme:

```toml
[[theme]]
name = "network-ops"
inherits = "hubuum-dark"

[theme.roles]
key = { fg = "ansi:cyan" }
table_header = { fg = "ansi:cyan", bold = true }
title = { fg = "ansi:green", bold = true }
border = { fg = "ansi:bright-black" }
message = { fg = "ansi:green" }
number = { fg = "ansi:cyan" }
boolean_false = { fg = "ansi:yellow" }
cidr = { fg = "ansi:magenta" }
url = { fg = "ansi:blue" }
value = {}

[theme.classifiers]
disabled = ["date"]
command_prefixes = ["hubuum-cli", "osp"]
```

By default all classifiers are enabled and the command prefix is `hubuum-cli`.
Prefix matching is case-sensitive and requires whitespace or the end of the
value after the prefix, so `hubuum-client` is ordinary text. Prefixes must be
nonempty, trimmed, and free of control characters. An empty prefix list disables
command recognition.

`disabled` accepts category names from the table (`ordinary` names the fallback
category). A disabled category falls back to ordinary styling without trying
another classifier. Unknown names, including `muted`, are rejected.

Child themes inherit classifier settings when they omit `[theme.classifiers]`.
Providing that table replaces the inherited settings; omitted fields then use
their defaults. Role overrides inherit individually, and changing `number`
updates all categories that still use its default style.
