# Rust bluepill wireless projects

This repo contains several projects

### Libraries
- `shared` (`./projects/shared`)

### Applciations
- `controller` (`./projects/devices/controller`)
- `lights` (`./projects/devices/lights`)

## Running

First, start OpenOCD. Then, run the project of choice with the `-p` flag. For example:

```sh
cargo run -p lights
```
