# Rust nativeness scorecard

RustyGet started as a line-by-line port of the original C++ code. It now builds as pure
Rust, but much of it still *reads* like C++: integer status codes, `extern "C"`
shims between Rust modules, getters and setters, out-parameters, panics. The
scorecard turns that impression into numbers so idiomatic-Rust work can be
tracked, compared between PRs and split by module.

```sh
just nativeness                  # full run with clippy, compared to the baseline
scripts/nativeness/score.py      # text signals only, a few seconds
scripts/nativeness/score.py --run-clippy --json out.json --markdown out.md \
    --compare docs/nativeness/baseline.json
```

The `nativeness` workflow runs the full version on every push to `main` and on
pull requests that touch `rust/`, the scorecard or its docs, and puts the
report in the job summary. It is informational and not
a required check.

## What is counted

Only non-test library code under `rust/src` counts. Items, fields, variants
and match arms under `#[cfg(test)]` (or `cfg(all(.., test, ..))`), files
reached only through a `#[cfg(test)] mod name;` declaration, `tests.rs` files
and anything under a `tests/` directory are skipped. Comments
and string literals are blanked before matching. A module is a top-level file
plus everything in its directory (`progressive_unpack.rs` and
`progressive_unpack/*`).

| Category | Weight | Signals (multiplier) | Idiomatic fix |
|---|---|---|---|
| FFI and C ABI leftovers | 15 | `extern "C" fn` / block (3), `no_mangle` (3), raw pointer types (1), `c_int`/`c_char`/`CStr`/`CString` (0.5), `Box::into_raw`/`from_raw`/`leak`, `.as_ptr()` (1) | Plain Rust functions taking references, owned types or `&[u8]`; no C ABI between Rust modules |
| unsafe | 10 | each `unsafe` keyword | Safe wrappers; delete unsafe that only existed for the C ABI |
| Integers where enums belong | 15 | integer fields/params named like codes (`status`, `kind`, `mode`, `op`, `*_kind`...) (2), integer-literal match arms (1), `-1` sentinels (2), non-count functions returning `i32`/`c_int` (1) | `enum`s, `Option`, `Result` |
| C++ API shape | 10 | `get_*`/`set_*` methods (1), `&mut` out-parameters on functions returning `()`/`bool`/int (2), 7+ parameters (1), `&String`/`&Vec`/`&Box`/`&PathBuf` params (1) | Field access or plain-noun getters, return tuples or structs, parameter structs, `&str`/`&[T]`/`&Path` |
| Error handling | 10 | `bool`-returning action functions (`load_`, `open_`, `write_`... ) (1), `Result<_, String/()/i32>` (1) | `Result<T, E>` with a typed error and `?` |
| Panics in library code | 10 | `.unwrap()` except on `lock()/read()/write()/join()` (1), `.expect(` (0.5), `panic!`/`unreachable!`/`todo!` (1) | Propagate with `?`, or document the invariant |
| Numeric `as` casts | 5 | `as` to a numeric or C type (1) | `From`/`TryFrom`, typed fields |
| Index loops | 5 | `for i in 0..x.len()`, `while i < x.len()` (1) | Iterators, `enumerate`, `zip`, `windows` |
| Global mutable state | 5 | `static mut` (3), static `Mutex`/`RwLock`/`OnceLock`/atomics (1), `thread_local!` (1) | State owned by a struct and passed in |
| Size | 5 | functions over 100 lines (1), structs with 20+ fields (2) | Split by responsibility |
| C-style naming and paths as strings | 5 | `allow(non_snake_case...)` (3), `m_` identifiers (1), path-like names typed `&str`/`String` (0.5) | snake_case, `Path`/`PathBuf` |
| Clippy | 15 | warnings from default, `pedantic` and `nursery`, minus pure doc/attribute lints | Whatever the lint says |

The clippy run passes `-W clippy::correctness` so the few deny-by-default
lints are counted instead of stopping the run. A clippy run that does not end
in a successful build (a compile error, a bad manifest) is refused rather than
scored, since missing warnings would read as an improvement. Lints ignored for scoring
(still listed in the report): `missing_errors_doc`, `missing_panics_doc`,
`must_use_candidate`, `doc_markdown`, `module_name_repetitions`,
`missing_const_for_fn`, `too_long_first_doc_paragraph`, `doc_link_with_quotes`,
`struct_field_names`, `similar_names`, `items_after_statements`,
`redundant_pub_crate`.

## How the score is built

For each category, the weighted count is turned into a density per 1,000 code
lines, and the sub-score is `100 / (1 + density / half)`. Zero problems scores
100, a module at the category's `half` density scores 50, and the score never
goes negative however bad it gets. The overall and per-module scores are the
weighted mean of the sub-scores. The `half` values in `CATEGORIES` in
`score.py` are calibration constants; changing them resets the trend line, so
regenerate the baseline in the same PR and say so.

**Debt** is `(100 - module score) x KLOC`. It ranks where work pays off: a
3,000-line module at 45 outranks a 100-line module at 30. The report lists the
top modules by debt and, separately, the lowest scores among modules of 300+
lines.

Scores with and without clippy data are not comparable (the clippy category is
dropped and weights renormalise when it is missing). `--compare` against a
report made the other way is ignored with a note, and `--fail-on-drop` refuses
to run. `just nativeness no` therefore prints no deltas.

## Tracking progress

`baseline.json` and `baseline.md` here are the first full run, on `main` at
`4279b35` (2026-11-19). Pass `--compare docs/nativeness/baseline.json` to see
deltas; improvements are marked with a check. Refresh the baseline when a
milestone lands, not on every PR, so deltas stay meaningful.

`--fail-under N` and `--fail-on-drop N` exist for a future ratchet: once the
score is moving, CI can fail a PR that lowers it by more than `N`.

## Limits

The analysis is lexical, not a parser. Counts are close but not exact:
integer-literal match arms include genuine byte parsing, `#[cfg(test)] mod x;` inside an inline module or with `#[path]` is not resolved, `bool` action
functions include a few legitimate ones, and a renamed C type escapes the
`c_types` count. The aim is a stable trend line and a ranking, not a proof.
Tools considered: `cargo-geiger` counts unsafe in dependencies too (ours is
crate-local and covered here), and `rust-code-analysis` gives complexity
metrics, which clippy's `too_many_lines` and `cognitive_complexity` already
approximate. Neither is needed to run the scorecard.
