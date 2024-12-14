# `mise doctor path`

- **Usage**: `mise doctor path [-f --full]`
- **Source code**: [`src/cli/doctor/path.rs`](https://github.com/jdx/mise/blob/main/src/cli/doctor/path.rs)

Print the current PATH entries mise is providing

## Flags

### `-f --full`

Print all entries including those not provided by mise

Examples:

    Get the current PATH entries mise is providing
    $ mise path
    /home/user/.local/share/mise/installs/node/24.0.0/bin
    /home/user/.local/share/mise/installs/rust/1.90.0/bin
    /home/user/.local/share/mise/installs/python/3.10.0/bin
