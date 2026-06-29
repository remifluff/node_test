# node_test

Pd-style patch editor built with [egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui/tree/master/crates/eframe).

## Workspace

| Crate | Role |
|-------|------|
| `patch_graph` | Node graph domain model, `.lop` export, and automatic layout (merged from the old `patch_layout` crate) |
| `mouse_ui` | Shared canvas, styling, and mouse-driven node widgets |
| `keyboard_ui` | Sorted-layout pane with arrow-key navigation |
| `node_test` (`pd_editor` binary) | App shell composing the two UI panes |

## Run

```sh
cargo run --bin pd_editor
```

## Tests

```sh
cargo test -p patch_graph   # layout engine
cargo test                   # editor integration tests
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
