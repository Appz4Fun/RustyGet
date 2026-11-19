# Rust nativeness scorecard

**Score: 37.5 / 100**  
174,233 non-test code lines in 119 modules. Higher is more idiomatic; see `docs/nativeness/README.md` for what each signal means.

## Headline numbers

| Signal | Value |
|---|---|
| code lines | 174233 |
| extern c fns | 334 |
| unsafe per kloc | 15.13 |
| int code fields | 859 |
| magic int match arms | 1295 |
| library unwraps | 712 |
| get set accessors | 195 |
| out params | 260 |
| question marks per kloc | 18.78 |
| clippy warnings per kloc | 34.83 |

## Categories

| Category | Weight | Count (weighted) | Per KLOC | Score |
|---|---|---|---|---|
| FFI and C ABI leftovers | 15 | 6265.0 | 35.96 | 10.0 |
| unsafe | 10 | 2636 | 15.13 | 11.7 |
| Integers where enums belong | 15 | 3566 | 20.47 | 22.7 |
| C++ API shape (get_/set_, out-params, long param lists) | 10 | 1202 | 6.9 | 30.3 |
| Error handling (bool/int status, stringly errors) | 10 | 317 | 1.82 | 68.7 |
| Panics in library code (unwrap/expect/panic!) | 10 | 744.0 | 4.27 | 41.3 |
| Numeric `as` casts | 5 | 3226 | 18.52 | 44.8 |
| Index loops instead of iterators | 5 | 158 | 0.91 | 52.4 |
| Global mutable state | 5 | 192 | 1.1 | 47.6 |
| Size (long fns, giant structs) | 5 | 325 | 1.87 | 44.6 |
| C-style naming and paths as strings | 5 | 122.5 | 0.7 | 74.0 |
| Clippy warnings (default+pedantic+nursery, doc lints excluded) | 15 | 6068 | 34.83 | 53.5 |

## Where the work pays off (top 25 by debt)

Debt = (100 - module score) x KLOC, so a big C-shaped module outranks a tiny one. Fixing the top of this list moves the overall score most.

| Module | Score | Lines | Debt share | Weakest categories | Biggest raw signals |
|---|---|---|---|---|---|
| `diskstate_format` | 42.9 | 8,150 | 6.0% | unsafe 3, ffi 4, int_codes 18 | unsafe 611, c_types 598, raw_ptr 301, into_from_raw 113 |
| `streamrepair` | 62.8 | 10,302 | 5.0% | casts 30, api_shape 35, loops 35 | as_cast 363, bool_status_fn 39, allow_clippy 29, many_params 27 |
| `ffi` | 45.9 | 6,727 | 4.7% | ffi 4, unsafe 8, int_codes 15 | raw_ptr 268, c_types 212, unsafe 160, as_cast 126 |
| `dupesearch_coord` | 47.5 | 6,290 | 4.3% | ffi 11, unsafe 25, size 39 | c_types 133, raw_ptr 120, as_cast 70, unsafe 37 |
| `xmlrpc_format` | 46.6 | 5,765 | 4.0% | unsafe 5, ffi 6, int_codes 15 | c_types 380, unsafe 216, raw_ptr 146, magic_int_arm 69 |
| `parchecker` | 53.7 | 6,036 | 3.6% | panics 5, ffi 30, int_codes 41 | unwrap 330, as_cast 109, c_types 32, raw_ptr 21 |
| `purgejournal` | 71.1 | 8,526 | 3.2% | panics 27, int_codes 37, size 43 | as_cast 118, unwrap 69, int_code_field 36, magic_int_arm 16 |
| `options` | 44.9 | 4,297 | 3.1% | ffi 6, int_codes 20, api_shape 24 | c_types 367, int_code_field 36, raw_ptr 30, as_cast 27 |
| `queuesubsys_host` | 49.8 | 4,117 | 2.7% | ffi 7, unsafe 10, int_codes 18 | raw_ptr 128, c_types 109, as_cast 96, unsafe 74 |
| `net` | 42.6 | 3,509 | 2.6% | ffi 5, unsafe 8, int_codes 16 | c_types 237, raw_ptr 94, unsafe 77, as_cast 47 |
| `postproc` | 56.3 | 4,511 | 2.6% | api_shape 22, int_codes 24, globals 33 | magic_int_arm 39, as_cast 24, int_code_field 18, expect 18 |
| `articledl` | 46.6 | 3,479 | 2.4% | unsafe 12, ffi 19, globals 20 | as_cast 59, unsafe 49, c_types 41, raw_ptr 25 |
| `binrpc` | 40.5 | 2,996 | 2.3% | panics 11, unsafe 15, int_codes 17 | as_cast 143, unwrap 73, magic_int_arm 35, unsafe 33 |
| `queuecoord` | 70.2 | 5,785 | 2.2% | api_shape 21, size 44, globals 54 | as_cast 73, out_param 21, many_params 19, allow_clippy 14 |
| `dupesearch_nzb` | 57.6 | 3,807 | 2.1% | naming 8, loops 18, int_codes 24 | m_prefix 83, as_cast 82, magic_int_arm 29, int_status_fn 21 |
| `historycoord` | 48.3 | 2,988 | 2.0% | ffi 6, int_codes 14, unsafe 18 | c_types 135, raw_ptr 70, magic_int_arm 69, as_cast 37 |
| `qnzb` | 38.9 | 2,386 | 1.9% | unsafe 2, ffi 3, int_codes 5 | magic_int_arm 233, unsafe 217, c_types 166, raw_ptr 159 |
| `feed` | 47.0 | 2,682 | 1.8% | ffi 7, unsafe 15, int_codes 16 | c_types 99, raw_ptr 44, as_cast 39, unsafe 31 |
| `ncurses_format` | 44.7 | 2,457 | 1.8% | ffi 8, api_shape 11, unsafe 13 | as_cast 82, c_types 64, magic_int_arm 42, unsafe 34 |
| `systemhealth` | 62.6 | 3,390 | 1.6% | ffi 23, api_shape 27, int_codes 34 | c_types 33, raw_ptr 13, int_code_field 13, magic_int_arm 12 |
| `scanner` | 45.9 | 2,063 | 1.4% | ffi 3, unsafe 6, int_codes 17 | c_types 193, raw_ptr 75, unsafe 61, into_from_raw 35 |
| `lifecycle` | 43.8 | 1,898 | 1.4% | ffi 4, unsafe 8, int_codes 14 | c_types 107, magic_int_arm 52, unsafe 44, raw_ptr 30 |
| `dupefallback` | 64.2 | 2,771 | 1.3% | int_codes 22, api_shape 22, casts 34 | as_cast 82, many_params 14, sentinel_minus_one 12, int_status_fn 11 |
| `statmeter` | 42.7 | 1,601 | 1.2% | ffi 7, unsafe 12, api_shape 17 | as_cast 61, c_types 58, raw_ptr 31, magic_int_arm 26 |
| `cmdline` | 42.8 | 1,583 | 1.2% | ffi 4, unsafe 6, globals 18 | c_types 139, unsafe 53, raw_ptr 40, as_cast 39 |

## Lowest scores (modules of 300+ lines)

| Module | Score | Lines | Weakest categories | Biggest raw signals |
|---|---|---|---|---|
| `socket` | 36.9 | 1,132 | ffi 7, api_shape 8, unsafe 10 | c_types 71, unsafe 20, raw_ptr 15, out_param 14 |
| `qnzb` | 38.9 | 2,386 | unsafe 2, ffi 3, int_codes 5 | magic_int_arm 233, unsafe 217, c_types 166, raw_ptr 159 |
| `qfile` | 39.3 | 1,072 | unsafe 2, ffi 3, int_codes 6 | magic_int_arm 88, unsafe 83, as_cast 80, c_types 79 |
| `binrpc` | 40.5 | 2,996 | panics 11, unsafe 15, int_codes 17 | as_cast 143, unwrap 73, magic_int_arm 35, unsafe 33 |
| `tlssock` | 40.5 | 1,243 | unsafe 2, ffi 2, int_codes 13 | unsafe 151, c_types 125, raw_ptr 71, into_from_raw 34 |
| `log` | 42.2 | 788 | ffi 4, unsafe 6, int_codes 10 | c_types 45, as_cast 34, unsafe 25, raw_ptr 21 |
| `net` | 42.6 | 3,509 | ffi 5, unsafe 8, int_codes 16 | c_types 237, raw_ptr 94, unsafe 77, as_cast 47 |
| `util` | 42.6 | 1,308 | loops 9, unsafe 18, casts 19 | as_cast 86, c_types 24, index_loop 13, unsafe 12 |
| `statmeter` | 42.7 | 1,601 | ffi 7, unsafe 12, api_shape 17 | as_cast 61, c_types 58, raw_ptr 31, magic_int_arm 26 |
| `cmdline` | 42.8 | 1,583 | ffi 4, unsafe 6, globals 18 | c_types 139, unsafe 53, raw_ptr 40, as_cast 39 |
| `fsops` | 42.8 | 1,005 | unsafe 3, ffi 6, api_shape 13 | unsafe 65, c_types 53, into_from_raw 32, as_cast 28 |
| `diskstate_format` | 42.9 | 8,150 | unsafe 3, ffi 4, int_codes 18 | unsafe 611, c_types 598, raw_ptr 301, into_from_raw 113 |
| `nntp` | 43.6 | 1,280 | int_codes 10, unsafe 11, ffi 19 | int_code_field 29, c_types 27, unsafe 20, as_cast 16 |
| `lifecycle` | 43.8 | 1,898 | ffi 4, unsafe 8, int_codes 14 | c_types 107, magic_int_arm 52, unsafe 44, raw_ptr 30 |
| `memory_pressure` | 44.5 | 665 | ffi 9, globals 14, unsafe 25 | as_cast 23, raw_ptr 12, c_types 12, unsafe 4 |
| `nzbfile` | 44.6 | 1,484 | ffi 13, api_shape 14, int_codes 15 | c_types 46, as_cast 27, sentinel_minus_one 17, out_param 13 |
| `ncurses_format` | 44.7 | 2,457 | ffi 8, api_shape 11, unsafe 13 | as_cast 82, c_types 64, magic_int_arm 42, unsafe 34 |
| `procrun` | 44.8 | 1,063 | unsafe 6, ffi 7, int_codes 15 | c_types 43, unsafe 31, raw_ptr 12, into_from_raw 11 |
| `options` | 44.9 | 4,297 | ffi 6, int_codes 20, api_shape 24 | c_types 367, int_code_field 36, raw_ptr 30, as_cast 27 |
| `ffi` | 45.9 | 6,727 | ffi 4, unsafe 8, int_codes 15 | raw_ptr 268, c_types 212, unsafe 160, as_cast 126 |
| `qarticle` | 45.9 | 518 | ffi 3, unsafe 4, int_codes 16 | c_types 36, unsafe 28, as_cast 26, raw_ptr 24 |
| `scanner` | 45.9 | 2,063 | ffi 3, unsafe 6, int_codes 17 | c_types 193, raw_ptr 75, unsafe 61, into_from_raw 35 |
| `articledl` | 46.6 | 3,479 | unsafe 12, ffi 19, globals 20 | as_cast 59, unsafe 49, c_types 41, raw_ptr 25 |
| `xmlrpc_format` | 46.6 | 5,765 | unsafe 5, ffi 6, int_codes 15 | c_types 380, unsafe 216, raw_ptr 146, magic_int_arm 69 |
| `feed` | 47.0 | 2,682 | ffi 7, unsafe 15, int_codes 16 | c_types 99, raw_ptr 44, as_cast 39, unsafe 31 |

## Clippy

By group: pedantic 7,631, nursery 1,727, default 277, correctness 4

| Lint | Count | Scored |
|---|---|---|
| `must_use_candidate` | 1,435 | no |
| `cast_possible_truncation` | 993 | yes |
| `cast_sign_loss` | 807 | yes |
| `cast_possible_wrap` | 725 | yes |
| `redundant_closure_for_method_calls` | 609 | yes |
| `doc_markdown` | 570 | no |
| `missing_const_for_fn` | 527 | no |
| `redundant_pub_crate` | 397 | no |
| `cast_lossless` | 372 | yes |
| `too_many_lines` | 235 | yes |
| `too_long_first_doc_paragraph` | 227 | no |
| `borrow_as_ptr` | 214 | yes |
| `missing_errors_doc` | 169 | no |
| `map_unwrap_or` | 163 | yes |
| `option_if_let_else` | 155 | yes |
| `items_after_statements` | 150 | no |
| `significant_drop_tightening` | 145 | yes |
| `use_self` | 137 | yes |
| `unreadable_literal` | 136 | yes |
| `ptr_as_ptr` | 82 | yes |
