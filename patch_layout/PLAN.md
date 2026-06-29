# patch_layout — automatic patch organization

## Purpose

`patch_layout` sits **above** the editor's node graph and reorganizes **positions only**.
Topology (nodes, edges, ports, object data) is unchanged. The editor converts
`PatchGraph` → `LayoutGraph`, runs a layout pass, then writes new `pos` values back.

```
  ┌─────────────┐     LayoutGraph      ┌──────────────┐     positions     ┌─────────────┐
  │  pd_editor  │ ──────────────────►  │ patch_layout │ ────────────────► │  pd_editor  │
  │ PatchGraph  │                      │   engine     │   HashMap<id,Pt>  │  apply pos  │
  └─────────────┘                      └──────────────┘                   └─────────────┘
```

## Design principles

1. **UI-agnostic** — no `egui`, no `daggy`. Only topology + sizes + layout hints.
2. **Stable ids** — layout uses opaque node keys the editor maps to `NodeIndex`.
3. **Composable pipeline** — rank → order → assign → snap → post-process (delay pairs).
4. **Deterministic** — same graph + config → same layout (seed only for tie-breaks).
5. **Incremental-ready** — v0 relayouts selection or whole patch; later, pin nodes.

## Sorting mode goals (editor split view)

The organized / **sorted layout** pane runs `patch_layout` on the live graph via
[`rust-sugiyama`](https://crates.io/crates/rust-sugiyama) (layered Sugiyama layout)
plus a port-alignment post-pass. Two objectives, in priority order:

1. **Straight vertical cables** — align outlet and inlet X on every edge so patch cords run vertically (not diagonally).
2. **Grid placement** — snap spine columns and row Y to `grid_step` (15 px, matching the editor). Port-locked child X follows parent alignment rather than forcing wires off-grid.

Disconnected chains land in separate grid columns; dual-inlet combiners stretch horizontally so each inlet meets its feeder outlet.

## Core types (`graph.rs`)

| Type | Role |
|------|------|
| `LayoutGraph` | Nodes + directed edges (DAG assumed) |
| `LayoutNode` | `id`, `size`, `kind`, optional `group` (delay pair hex id) |
| `LayoutEdge` | `from`, `from_port`, `to`, `to_port` |
| `NodeKind` | Hints: `Source`, `Sink`, `Param`, `Combine`, `DelayIn`, `DelayOut`, … |
| `Point` | Output `(x, y)` in world/patch space |

## Configuration (`config.rs`)

| Field | Default | Meaning |
|-------|---------|---------|
| `column_spacing` | 120 | Horizontal gap between layers |
| `row_spacing` | 60 | Vertical gap within a layer |
| `grid_step` | 15 | Snap grid (matches editor `GRID_STEP`) |
| `origin` | (0, 0) | Top-left anchor for laid-out patch |
| `flow` | LeftToRight | Primary signal direction |

## Layout pipeline (planned)

### Phase 1 — Layer assignment (rank)

- Treat edges as downstream flow: outlet → inlet.
- Rank = longest path from any **source** (`In`, `DelayIn`, zero in-degree).
- `Combine` / `Param` follow graph topology like any other node.
- **Delay pairs**: `DelayOut` and matching `DelayIn` share a `group` id but are
  **not** connected; rank `DelayOut` with its upstream cluster and `DelayIn` with
  its downstream cluster (they may land in different columns — intentional).

### Phase 2 — Layer ordering (reduce crossings)

- Barycenter / median heuristic sweeps (Sugiyama-style).
- Port index used as tie-break when ordering siblings fed by the same parent.
- `Combine` nodes pulled toward the centroid of their two inputs.

### Phase 3 — Coordinate assignment

- `x = origin.x + rank * column_spacing`
- `y = origin.y + slot_in_layer * row_spacing`
- Center node box on grid; snap all positions to `grid_step`.

### Phase 4 — Post-processing

| Rule | Behavior |
|------|----------|
| Delay pair | After global layout, move `DelayOut` below cycle bbox bottom, `DelayIn` above top (reuse cycle detection from editor logic, ported here) |
| Comments | Separate pass: float near associated node or bottom strip |
| Selection-only | Filter subgraph, layout, merge back with offset |

## Editor integration (not in this crate)

```rust
// pd_editor: layout_adapter.rs (future)
fn patch_to_layout(graph: &PatchGraph, pairs: &DelayPairMap) -> LayoutGraph { … }
fn apply_layout(graph: &mut PatchGraph, result: &LayoutResult) { … }
```

Trigger: menu item **Organize patch** / shortcut; optional auto-organize on load.

## Module roadmap

| Version | Deliverable |
|---------|-------------|
| **v0** (now) | Types, config, `LayeredDagLayout` rank + column assignment, grid snap |
| **v1** | Crossing minimization, `Combine` centroid nudging |
| **v2** | Delay-pair post-pass, comment placement |
| **v3** | Partial / selection layout, pinned nodes |
| **v4** | Wire-length refinement (force-directed polish on fixed ranks) |

## Testing strategy

- Unit tests: small DAGs with known expected ranks and relative order.
- Snapshot tests: `(nodes, edges) → positions` for demo patch and cycle+break cases.
- Property: layout never changes edge endpoints; every edge still valid after apply.

## Open questions

1. Should `[out]` nodes be forced to the rightmost column regardless of rank?
2. Multi-component patches: lay out each weakly connected component separately with horizontal gap?
3. Preserve user nudge: store "pinned" flag on `Node` in editor vs layout crate?
