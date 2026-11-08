# Third-party notices

Strata's original source code is available under the [MIT License](LICENSE). Third-party components retain their own copyright notices and licenses.

## Bundled fonts

| Font | Copyright notice |
|---|---|
| Instrument Sans | Copyright 2022 The Instrument Sans Project Authors |
| Spline Sans Mono | Copyright 2022 The Spline Sans Mono Project Authors |
| Bricolage Grotesque | Copyright 2022 The Bricolage Grotesque Project Authors |

These fonts use the SIL Open Font License, Version 1.1. The full license and upstream project links are in [assets/fonts/OFL.txt](assets/fonts/OFL.txt). Release packages include that text in `licenses/FONTS-OFL.txt`.

## Database logos

The database marks in `assets/db` come from [Simple Icons](https://simpleicons.org), published under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). The SQL Server mark comes from version 10.4.0; the other marks come from version 13.21.0. See [the asset notes](assets/db/README.md) for the modifications made for Strata.

The marks are trademarks of their respective owners. Their use identifies compatible database engines and does not imply endorsement.

## Rust dependencies

Dependency versions are recorded in `Cargo.lock`. Each crate retains its own license; the Strata MIT license does not replace those terms. This document lists bundled assets, not a complete dependency license inventory.

To inspect the resolved crates and their declared licenses:

```sh
cargo metadata --locked --format-version 1 > dependency-metadata.json
```

The metadata includes each package's `license`, `license_file`, repository, and manifest path. Refer to each package's source distribution for its complete notices, including any bundled native libraries.
