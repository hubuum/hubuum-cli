# Server compatibility

Hubuum CLI, `hubuum_client`, and the Hubuum server are versioned independently.
A CLI release targets the server release declared and reproducibly tested by its
bundled `hubuum_client` version. This is a compatibility target, not a guarantee
that every CLI command is available against other server versions.

## Compatibility matrix

| CLI version | `hubuum_client` | Hubuum server target | Status |
| --- | --- | --- | --- |
| 0.0.3 | 0.6.1 | 0.0.3 | Current declared target |
| 0.0.2 | 0.5.1 | 0.0.2 | Previous declared target |
| 0.0.1 | 0.4.0 | `main@eed194f2339ce221ef251a14062e2a37850186b1` | Historical pre-release snapshot; no stable server target was declared |

The v0.0.3 target is tested by `hubuum_client` against the immutable
server image
`ghcr.io/hubuum/hubuum-server@sha256:f1f57a991f69005ee81f24e77533e61f75b5586949d98cccf1c40fc4329eb186`.
The v0.0.2 target was tested by `hubuum_client` against the immutable server image
`ghcr.io/hubuum/hubuum-server@sha256:8f543383b422124546c8d337fd557e1b182b1b6c7078d7870d3c5cd4f955ef1f`.
The v0.0.1 row records the reproducible server snapshot inherited from
`hubuum_client` v0.4.0; it predates the first stable CLI/server compatibility target.

Forward-compatibility checks against the server's `main` branch are useful early
warnings, but they do not change a published CLI release's declared target.
