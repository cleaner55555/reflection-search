# Reflection Search (radni naziv)

Besplatan meta-pretraživač. Plan: `PLAN.md`. Standard kvaliteta: §10 plana (clippy čist, fmt čist, coverage ≥ 80%).

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all --check
```

Config samo iz env (vidi `.env.example`).
