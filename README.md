# node_test

Pd-style patch editor built with [egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui/tree/master/crates/eframe), plus a UI-agnostic automatic layout library for directed acyclic patch graphs.

## Crates

| Crate | Role |
|-------|------|
| `node_test` | Desktop app (`pd_editor` binary) — graph editing, rendering, patch export |
| `patch_layout` | Layout engine — ranks and positions nodes without changing topology |

## Run

```sh
cargo run --bin pd_editor
```

## Layout

`patch_layout` converts a patch graph into coordinates only. See [`patch_layout/PLAN.md`](patch_layout/PLAN.md) for the design.

```sh
cargo test -p patch_layout
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
