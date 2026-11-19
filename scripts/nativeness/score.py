#!/usr/bin/env python3
"""Score how idiomatic ("native") RustyGet's Rust is, per module and overall.

The crate began as a line-by-line port of nzbget's C++. This script counts
signals that tell C++-ported Rust apart from idiomatic Rust (FFI leftovers,
integer status codes, getter/setter pairs, out-parameters, panics, casts,
index loops, global state, giant functions, string-typed paths) and, when
given clippy's JSON output, clippy warnings per lint group. Each category
becomes a 0-100 sub-score from its density per 1,000 lines of code, and a
weighted mean of the sub-scores is the headline score. Higher is better.

Only non-test library code counts: `#[cfg(test)]` items, `tests.rs` files and
anything under a `tests/` directory are skipped, because unwrap and fixture
casts are fine in tests.

Usage:
  scripts/nativeness/score.py                       # text signals only
  scripts/nativeness/score.py --run-clippy          # also run clippy (slow)
  scripts/nativeness/score.py --clippy-json c.json  # reuse a clippy run
  scripts/nativeness/score.py --json out.json --markdown out.md \
      --compare docs/nativeness/baseline.json

Python 3.9+, standard library only. The analysis is lexical (comments and
string literals are blanked first), so counts are close but not exact; the
point is a stable trend line, not a proof.
"""
import argparse
import json
import math
import re
import subprocess
import sys
from pathlib import Path

SCHEMA = 1

# --------------------------------------------------------------------------
# Lexing helpers
# --------------------------------------------------------------------------


def blank_comments_and_strings(src):
    """Return src with comments and string/char literal contents replaced by
    spaces. Newlines are kept so line numbers still line up."""
    out = []
    i, n = 0, len(src)

    def keep_newlines(s):
        return ''.join('\n' if c == '\n' else ' ' for c in s)

    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ''
        if c == '/' and nxt == '/':
            j = src.find('\n', i)
            j = n if j < 0 else j
            out.append(' ' * (j - i))
            i = j
        elif c == '/' and nxt == '*':
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith('/*', j):
                    depth, j = depth + 1, j + 2
                elif src.startswith('*/', j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            out.append(keep_newlines(src[i:j]))
            i = j
        elif c in 'rb' and re.match(r'(?:br|r)#*"', src[i:i + 260]) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == '_')):
            m = re.match(r'(?:br|r)(#*)"', src[i:])
            hashes = m.group(1)
            start = i + m.end()
            end = src.find('"' + hashes, start)
            end = n if end < 0 else end + 1 + len(hashes)
            out.append('"' + keep_newlines(src[i + 1:end - 1]) + '"')
            i = end
        elif c == '"' or (c == 'b' and nxt == '"' and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == '_'))):
            j = i + (2 if c == 'b' else 1)
            while j < n and src[j] != '"':
                j += 2 if src[j] == '\\' else 1
            j = min(j + 1, n)
            out.append('"' + keep_newlines(src[i + 1:j - 1]) + '"')
            i = j
        elif c == "'":
            # char literal ('a', '\n', '\u{1F600}', b'x') vs lifetime ('a)
            m = re.match(r"'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'\n])'", src[i:i + 12])
            if m:
                out.append("' '")
                i += m.end()
            else:
                out.append(c)
                i += 1
        else:
            out.append(c)
            i += 1
    return ''.join(out)


def match_brace(code, open_idx, open_ch='{', close_ch='}'):
    """Index just past the bracket matching code[open_idx]."""
    depth = 0
    for j in range(open_idx, len(code)):
        ch = code[j]
        if ch == open_ch:
            depth += 1
        elif ch == close_ch:
            depth -= 1
            if depth == 0:
                return j + 1
    return len(code)


CFG_TEST_PAT = r'#\[cfg\((?:test|all\((?:[^\]]*?[(,]\s*)?test\s*[,)][^\]]*)\)\]'
CFG_TEST = re.compile(CFG_TEST_PAT)
ITEM_START = re.compile(r'(?:pub(?:\([\w:\s]+\))?\s+)?(?:(?:unsafe|async|const|default|extern(?:\s+"[^"]*")?)\s+)*'
                        r'(?:mod|fn|impl|struct|enum|union|trait|type|use|const|static|macro_rules!|extern)\b')


def blank_test_items(code):
    """Blank every item annotated #[cfg(test)] (usually `mod tests {..}`)."""
    while True:
        m = CFG_TEST.search(code)
        if not m:
            return code
        j = m.end()
        # skip further attributes and whitespace
        while True:
            rest = code[j:]
            ws = len(rest) - len(rest.lstrip())
            j += ws
            if code.startswith('#[', j):
                j = match_brace(code, j + 1, '[', ']')
                continue
            break
        if ITEM_START.match(code, j):
            brace = code.find('{', j)
            semi = code.find(';', j)
            if semi != -1 and (brace == -1 or semi < brace):
                end = semi + 1
            else:
                end = match_brace(code, brace) if brace != -1 else len(code)
        else:
            end = end_of_element(code, j)
        code = code[:m.start()] + ''.join('\n' if c == '\n' else ' ' for c in code[m.start():end]) + code[end:]


def end_of_element(code, j):
    """End of a field, variant, match arm or statement starting at j: through
    the next `,`/`;` at depth 0, before an unmatched closing bracket, or right
    after a `{..}` block that is not followed by more of the expression."""
    depth = 0
    n = len(code)
    k = j
    while k < n:
        ch = code[k]
        if ch in '([{':
            depth += 1
        elif ch in ')]}':
            if depth == 0:
                return k
            depth -= 1
            if depth == 0 and ch == '}':
                r = k + 1
                while r < n and code[r] in ' \t\r\n':
                    r += 1
                if r < n and code[r] in ',;':
                    return r + 1
                if r >= n or code[r] not in '.?)]':
                    return k + 1
        elif ch in ',;' and depth == 0:
            return k + 1
        k += 1
    return n


def split_top_level(s, sep=','):
    parts, depth, cur = [], 0, []
    for ch in s:
        if ch in '([{<':
            depth += 1
        elif ch in ')]}>':
            depth -= 1
        if ch == sep and depth == 0:
            parts.append(''.join(cur))
            cur = []
        else:
            cur.append(ch)
    if ''.join(cur).strip():
        parts.append(''.join(cur))
    return [p.strip() for p in parts if p.strip()]


# --------------------------------------------------------------------------
# Signals
# --------------------------------------------------------------------------

INT_T = r'(?:i8|i16|i32|i64|i128|isize|u8|u16|u32|u64|u128|usize|c_int|c_uint|c_long|c_ulong|c_short|c_uchar|c_char)'
CODE_NAMES = (r'(?:kind|status|state|mode|action|scenario|error|err|result|res|ret|rc|op|code|type|typ|ty|'
              r'flag|flags|level|stage|phase|reason|outcome|verdict|category|priority|severity|direction|'
              r'tier|method|strategy|policy|cmd|command|which|variant|class|style|format|fmt|choice|decision)')
CODE_FIELD = re.compile(r'(?<![\w.])(?:pub(?:\([\w:]+\))?\s+)?(?:\w+_)?' + CODE_NAMES + r'\s*:\s*' + INT_T + r'\b')
MAGIC_ARM = re.compile(r'^[ \t]*-?\d+(?:_\d+)*[ \t]*(?:\|[ \t]*-?\d+[ \t]*)*(?:\.\.=?[ \t]*-?\d+[ \t]*)?=>', re.M)
SENTINEL = re.compile(r'\breturn\s+-1\b|=>\s*-1\s*,|\bOk\(-1\)|\belse\s*\{\s*-1\s*\}')
# String literals are blanked before matching, so `extern "C"` reads `extern " "`.
EXTERN_C_FN = re.compile(r'\bextern\s+"\s{0,8}"\s+fn\b')
EXTERN_C_BLOCK = re.compile(r'\bextern\s+"\s{0,8}"\s*\{')
NO_MANGLE = re.compile(r'\bno_mangle\b')
RAW_PTR = re.compile(r'\*(?:const|mut)\s+[\w:<\[(]')
C_TYPES = re.compile(r'\b(?:c_int|c_uint|c_char|c_uchar|c_long|c_ulong|c_short|c_void|size_t|ssize_t|CStr|CString)\b')
INTO_RAW = re.compile(r'\b(?:Box|Arc|Rc|CString|Vec)::(?:into_raw|from_raw|leak)\b|\bas_ptr\(\)')
UNSAFE = re.compile(r'\bunsafe\b(?!\s*\()')  # not the #[unsafe(no_mangle)] attribute
UNWRAP = re.compile(r'\.unwrap\(\)')
LOCK_UNWRAP = re.compile(r'\.(?:lock|read|write|join)\(\)\s*\.unwrap\(\)')
EXPECT = re.compile(r'\.expect\(')
PANIC_MACRO = re.compile(r'\b(?:panic|unreachable|todo|unimplemented)!')
CAST = re.compile(r'\bas\s+(?:i8|i16|i32|i64|i128|isize|u8|u16|u32|u64|u128|usize|f32|f64|c_\w+)\b')
INDEX_LOOP = re.compile(r'\bfor\s+\w+\s+in\s+\(?\s*0\s*\.\.=?\s*[\w.()]+\.len\(\)|\bwhile\s+\w+\s*<\s*[\w.()]+\.len\(\)')
STATIC_MUT = re.compile(r'\bstatic\s+mut\b')
STATIC_SHARED = re.compile(r'\bstatic\s+\w+\s*:\s*(?:[\w:]+::)?(?:Mutex|RwLock|OnceLock|OnceCell|LazyLock|Lazy|Atomic\w+|[\w:]*Mutex|Condvar)\b')
THREAD_LOCAL = re.compile(r'\bthread_local!')
ALLOW_C_NAMES = re.compile(r'allow\((?:[^)]*\b)?(?:non_snake_case|non_camel_case_types|non_upper_case_globals)')
M_PREFIX = re.compile(r'\bm_[a-z]\w*')
ALLOW_CLIPPY = re.compile(r'allow\(\s*clippy::(\w+)')
REF_OWNED = re.compile(r':\s*&(?:mut\s+)?(?:String|Vec<|Box<|PathBuf)\b')
PATHY = r'(?:path|dir|directory|file|filename|filepath|dest|dst|src|folder|dirname|root|\w+_path|\w+_dir|\w+_file|\w+_filename|\w+_folder)'
PATH_AS_STR = re.compile(r'(?<![\w.])' + PATHY + r'\s*:\s*(?:&(?:\'\w+\s+)?str|String)\b')
RESULT_HEAD = re.compile(r'\bResult\s*<')
STRINGY_ERR = re.compile(r"^(?:String|\(\)|&(?:'\w+\s+)?str|i32|c_int)$")


def count_stringy_results(code):
    n = 0
    for m in RESULT_HEAD.finditer(code):
        end = match_angle(code, m.end() - 1)
        args = split_top_level(code[m.end():end - 1])
        if len(args) == 2 and STRINGY_ERR.match(args[1]):
            n += 1
    return n


def match_angle(code, open_idx):
    """Index past the `>` matching code[open_idx] == '<'; `->` is not a bracket."""
    depth = 0
    for k in range(open_idx, len(code)):
        ch = code[k]
        if ch == '<':
            depth += 1
        elif ch == '>' and code[k - 1] != '-':
            depth -= 1
            if depth == 0:
                return k + 1
        elif ch in ';{' and depth:
            return k
    return len(code)
QUESTION = re.compile(r'[\w)\]]\?(?=[\s.;),\]}])')
FN_HEAD = re.compile(r'\bfn\s+(\w+)\s*')
STRUCT_HEAD = re.compile(r'\bstruct\s+(\w+)\s*(?:<[^{;]*?>)?\s*(?:where[^{;]*)?\{')
# C++ `bool Load()` style: an action verb that reports success as a bool.
ACTION_BOOL = re.compile(r'^(?:load|save|read|write|open|close|init|initialize|start|stop|create|delete|remove|rename|move|copy|parse|process|send|recv|receive|connect|apply|execute|exec|run|do|handle|prepare|update|commit|flush|lock|unlock|register|add|insert|put|append|extract|unpack|download|fetch|decode|encode|repair|verify|cleanup|clean|make|build|try|restore|resume|finish|complete|bind|listen|accept|flush|sync|truncate|seek|alloc|allocate|reserve|reload|import|export|scan)(?:_|$)')
OUT_PARAM = re.compile(r'&mut\s+(?:bool|' + INT_T + r'|f32|f64|String|Vec<|Option<|usize|PathBuf)')

LONG_FN = 100        # lines
MANY_PARAMS = 7      # parameters, not counting self
BIG_STRUCT = 20      # named fields
BIG_MODULE = 3000    # non-test code lines in one file
MIN_RANKED_LOC = 300 # smaller modules are left out of the lowest-score table


def analyze_functions(code):
    fns = []
    for m in FN_HEAD.finditer(code):
        name = m.group(1)
        po = m.end()
        if code.startswith('<', po):
            po = match_angle(code, po)
            while po < len(code) and code[po] in ' \t\r\n':
                po += 1
        if not code.startswith('(', po):
            continue
        pc = match_brace(code, po, '(', ')')
        params = split_top_level(code[po + 1:pc - 1])
        # signature tail up to body or ;
        k = pc
        while k < len(code) and code[k] not in '{;':
            k += 1
        tail = code[pc:k]
        ret = ''
        rm = re.search(r'->\s*(.*?)(?:\bwhere\b|$)', tail, re.S)
        if rm:
            ret = rm.group(1).strip()
        body_lines = 0
        if k < len(code) and code[k] == '{':
            end = match_brace(code, k)
            body = code[k:end]
            body_lines = sum(1 for ln in body.split('\n') if ln.strip())
        non_self = [p for p in params if not re.match(r"^(?:&(?:'\w+\s+)?(?:mut\s+)?)?(?:mut\s+)?self\b", p)]
        fns.append({'name': name, 'params': non_self, 'ret': ret, 'lines': body_lines,
                    'extern_c': bool(re.search(r'extern\s+"\s{0,8}"\s*$', code[max(0, m.start() - 40):m.start()]))})
    return fns


def struct_field_counts(code):
    counts = []
    for m in STRUCT_HEAD.finditer(code):
        end = match_brace(code, m.end() - 1)
        body = code[m.end():end - 1]
        fields = [f for f in split_top_level(body) if re.match(r'^(?:#\[[^\]]*\]\s*)*(?:pub(?:\([\w:]+\))?\s+)?\w+\s*:', f)]
        counts.append((m.group(1), len(fields)))
    return counts


def scan_file(text):
    code = blank_test_items(blank_comments_and_strings(text))
    loc = sum(1 for ln in code.split('\n') if ln.strip())
    s = {}
    s['loc'] = loc
    s['extern_c_fn'] = len(EXTERN_C_FN.findall(code))
    s['extern_c_block'] = len(EXTERN_C_BLOCK.findall(code))
    s['no_mangle'] = len(NO_MANGLE.findall(code))
    s['raw_ptr'] = len(RAW_PTR.findall(code))
    s['c_types'] = len(C_TYPES.findall(code))
    s['into_from_raw'] = len(INTO_RAW.findall(code))
    s['unsafe'] = len(UNSAFE.findall(code))
    s['int_code_field'] = len(CODE_FIELD.findall(code))
    s['magic_int_arm'] = len(MAGIC_ARM.findall(code))
    s['sentinel_minus_one'] = len(SENTINEL.findall(code))
    lock_unwraps = len(LOCK_UNWRAP.findall(code))
    s['unwrap'] = len(UNWRAP.findall(code)) - lock_unwraps
    s['lock_unwrap'] = lock_unwraps
    s['expect'] = len(EXPECT.findall(code))
    s['panic_macro'] = len(PANIC_MACRO.findall(code))
    s['as_cast'] = len(CAST.findall(code))
    s['index_loop'] = len(INDEX_LOOP.findall(code))
    s['static_mut'] = len(STATIC_MUT.findall(code))
    s['static_shared'] = len(STATIC_SHARED.findall(code))
    s['thread_local'] = len(THREAD_LOCAL.findall(code))
    s['allow_c_naming'] = len(ALLOW_C_NAMES.findall(code))
    s['m_prefix'] = len(M_PREFIX.findall(code))
    allows = ALLOW_CLIPPY.findall(code)
    s['allow_clippy'] = len(allows)
    s['ref_owned_param'] = len(REF_OWNED.findall(code))
    s['path_as_str'] = len(PATH_AS_STR.findall(code))
    s['result_stringy_err'] = count_stringy_results(code)
    s['question_mark'] = len(QUESTION.findall(code))

    fns = analyze_functions(code)
    s['fn_count'] = len(fns)
    s['get_set_accessor'] = sum(1 for f in fns if re.match(r'^(?:get|set)_\w', f['name']))
    s['out_param'] = sum(sum(1 for p in f['params'] if OUT_PARAM.search(p.split(':', 1)[-1]))
                         for f in fns if re.fullmatch(r'(?:\(\)|bool|' + INT_T + r')?', f['ret']))
    s['bool_status_fn'] = sum(1 for f in fns if f['ret'] == 'bool' and ACTION_BOOL.match(f['name']))
    s['int_status_fn'] = sum(1 for f in fns if re.fullmatch(r'(?:i32|c_int)', f['ret']) and not f['extern_c']
                             and not re.search(r'(?:count|len|size|num|id|idx|index|max|min|total|sum|width|height|percent|pct|level|port|age|days|secs|millis|rate|speed|limit|priority|value|offset|pos|year|month|day|hour|minute|second)s?$', f['name']))
    s['long_fn'] = sum(1 for f in fns if f['lines'] > LONG_FN)
    s['many_params'] = sum(1 for f in fns if len(f['params']) >= MANY_PARAMS)
    s['big_struct'] = sum(1 for _, c in struct_field_counts(code) if c >= BIG_STRUCT)
    return s, allows


# --------------------------------------------------------------------------
# Scoring
# --------------------------------------------------------------------------

# Each category turns a weighted count into a density per 1,000 code lines.
# Sub-score = 100 / (1 + density / half), so a module at the `half` density
# scores 50, one at zero scores 100, and doubling the problems never drives a
# score negative. `half` values are calibration constants: roughly the density
# where a reviewer would call the module "still C-shaped". Change them only
# with a note in docs/nativeness/README.md, since that resets the trend line.
CATEGORIES = [
    # key, label, weight, half, {signal: multiplier}
    ('ffi', 'FFI and C ABI leftovers', 15, 4.0,
     {'extern_c_fn': 3, 'extern_c_block': 3, 'no_mangle': 3, 'raw_ptr': 1, 'c_types': 0.5, 'into_from_raw': 1}),
    ('unsafe', 'unsafe', 10, 2.0, {'unsafe': 1}),
    ('int_codes', 'Integers where enums belong', 15, 6.0,
     {'int_code_field': 2, 'magic_int_arm': 1, 'sentinel_minus_one': 2, 'int_status_fn': 1}),
    ('api_shape', 'C++ API shape (get_/set_, out-params, long param lists)', 10, 3.0,
     {'get_set_accessor': 1, 'out_param': 2, 'many_params': 1, 'ref_owned_param': 1}),
    ('errors', 'Error handling (bool/int status, stringly errors)', 10, 4.0,
     {'bool_status_fn': 1, 'result_stringy_err': 1}),
    ('panics', 'Panics in library code (unwrap/expect/panic!)', 10, 3.0,
     {'unwrap': 1, 'expect': 0.5, 'panic_macro': 1}),
    ('casts', 'Numeric `as` casts', 5, 15.0, {'as_cast': 1}),
    ('loops', 'Index loops instead of iterators', 5, 1.0, {'index_loop': 1}),
    ('globals', 'Global mutable state', 5, 1.0,
     {'static_mut': 3, 'static_shared': 1, 'thread_local': 1}),
    ('size', 'Size (long fns, giant structs)', 5, 1.5, {'long_fn': 1, 'big_struct': 2}),
    ('naming', 'C-style naming and paths as strings', 5, 2.0,
     {'allow_c_naming': 3, 'm_prefix': 1, 'path_as_str': 0.5}),
    ('clippy', 'Clippy warnings (default+pedantic+nursery, doc lints excluded)', 15, 40.0,
     {'clippy_scored': 1}),
]

# Pedantic lints that are about docs/attributes, not about the code's shape.
CLIPPY_IGNORED = {
    'missing_errors_doc', 'missing_panics_doc', 'must_use_candidate', 'doc_markdown',
    'module_name_repetitions', 'missing_const_for_fn', 'too_long_first_doc_paragraph',
    'doc_link_with_quotes', 'struct_field_names', 'similar_names', 'items_after_statements',
    'multiple_crate_versions', 'cargo_common_metadata', 'redundant_pub_crate',
}


def sub_score(density, half):
    return 100.0 / (1.0 + density / half)


def score_signals(sig, have_clippy):
    loc = max(sig.get('loc', 0), 1)
    cats = {}
    total_w = 0.0
    acc = 0.0
    for key, label, weight, half, mult in CATEGORIES:
        if key == 'clippy' and not have_clippy:
            continue
        weighted = sum(sig.get(k, 0) * m for k, m in mult.items())
        density = weighted * 1000.0 / loc
        sc = sub_score(density, half)
        cats[key] = {'label': label, 'weight': weight, 'weighted_count': round(weighted, 1),
                     'per_kloc': round(density, 2), 'score': round(sc, 1)}
        total_w += weight
        acc += weight * sc
    return round(acc / total_w, 1), cats


# --------------------------------------------------------------------------
# Clippy
# --------------------------------------------------------------------------


def run_clippy(rust_dir):
    cmd = ['cargo', 'clippy', '--lib', '--message-format=json', '--quiet', '--',
           '-W', 'clippy::pedantic', '-W', 'clippy::nursery',
           # deny-by-default correctness lints would stop the run; count them instead
           '-W', 'clippy::correctness']
    print('running: ' + ' '.join(cmd), file=sys.stderr)
    proc = subprocess.run(cmd, cwd=rust_dir, capture_output=True, text=True)
    if proc.returncode != 0:
        # A failed build means clippy never saw some code; scoring it would
        # read as an improvement.
        sys.stderr.write(proc.stderr[-4000:])
        raise SystemExit(f'clippy failed with exit {proc.returncode}; not scoring partial output')
    return proc.stdout.splitlines()


def require_complete_clippy(lines):
    """Refuse clippy output that does not end in a successful build."""
    for line in reversed(lines):
        line = line.strip()
        if line.startswith('{') and '"build-finished"' in line:
            try:
                if json.loads(line).get('success') is True:
                    return
            except ValueError:
                pass
            break
    raise SystemExit('clippy output has no successful build-finished record; '
                     'the run failed or was cut short, so it cannot be scored')


GROUP_NOTE = re.compile(r'(?:implied by `-W clippy::(\w+)`|#\[(?:warn|deny)\(clippy::[\w-]+\)\]` on by default)')


def parse_clippy(lines, src_root):
    """Return ({module: {lint: n}}, {lint: group})."""
    per_mod, groups = {}, {}
    for line in lines:
        line = line.strip()
        if not line.startswith('{'):
            continue
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get('reason') != 'compiler-message':
            continue
        m = msg.get('message') or {}
        code = (m.get('code') or {}).get('code') or ''
        if not code.startswith('clippy::'):
            continue
        lint = code[len('clippy::'):]
        for child in m.get('children') or []:
            g = GROUP_NOTE.search(child.get('message', ''))
            if g:
                groups[lint] = g.group(1) or 'default'
        spans = [s for s in m.get('spans') or [] if s.get('is_primary')]
        if not spans:
            continue
        fname = spans[0].get('file_name', '')
        mod = module_of_clippy_path(fname, src_root)
        if mod is None:
            continue
        per_mod.setdefault(mod, {})
        per_mod[mod][lint] = per_mod[mod].get(lint, 0) + 1
    return per_mod, groups


def module_of_clippy_path(fname, src_root):
    p = Path(fname)
    if not p.is_absolute():
        p = (src_root.parent / p)
    try:
        rel = p.resolve().relative_to(src_root.resolve())
    except ValueError:
        return None
    if is_test_path(rel):
        return None
    return module_name(rel)


# --------------------------------------------------------------------------
# Driver
# --------------------------------------------------------------------------


def is_test_path(rel):
    return rel.name == 'tests.rs' or 'tests' in rel.parts[:-1]


def module_name(rel):
    """Top-level module: `foo.rs` and everything under `foo/` are `foo`."""
    first = rel.parts[0]
    return first[:-3] if first.endswith('.rs') else first


SIGNAL_KEYS = None


def add(into, other):
    for k, v in other.items():
        into[k] = into.get(k, 0) + v


TEST_GATED_MOD = re.compile(CFG_TEST_PAT + r'\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([\w:]+\))?\s+)?mod\s+(\w+)\s*;')


def test_gated_files(src_root):
    """Files reached only through `#[cfg(test)] mod name;` declarations."""
    gated = set()
    for path in src_root.rglob('*.rs'):
        code = blank_comments_and_strings(path.read_text(encoding='utf-8', errors='replace'))
        names = TEST_GATED_MOD.findall(code)
        if not names:
            continue
        base = path.parent if path.name in ('lib.rs', 'main.rs', 'mod.rs') else path.parent / path.stem
        for name in names:
            gated.add((base / (name + '.rs')).resolve())
            gated.add((base / name).resolve())
    return gated


def build(src_root, clippy_lines):
    modules = {}
    allow_clippy = {}
    gated = test_gated_files(src_root)
    for path in sorted(src_root.rglob('*.rs')):
        rel = path.relative_to(src_root)
        if is_test_path(rel):
            continue
        rp = path.resolve()
        if rp in gated or any(parent in gated for parent in rp.parents):
            continue
        sig, allows = scan_file(path.read_text(encoding='utf-8', errors='replace'))
        mod = module_name(rel)
        m = modules.setdefault(mod, {'files': 0, 'signals': {}, 'max_file_loc': 0})
        m['files'] += 1
        m['max_file_loc'] = max(m['max_file_loc'], sig['loc'])
        add(m['signals'], sig)
        for a in allows:
            allow_clippy[a] = allow_clippy.get(a, 0) + 1
    for m in modules.values():
        m['signals']['big_module'] = 1 if m['max_file_loc'] > BIG_MODULE else 0

    have_clippy = clippy_lines is not None
    clippy_by_lint, clippy_groups = {}, {}
    if have_clippy:
        require_complete_clippy(clippy_lines)
        per_mod, clippy_groups = parse_clippy(clippy_lines, src_root)
        for mod, lints in per_mod.items():
            if mod not in modules:
                continue
            scored = sum(n for l, n in lints.items() if l not in CLIPPY_IGNORED)
            modules[mod]['signals']['clippy_scored'] = scored
            modules[mod]['signals']['clippy_all'] = sum(lints.values())
            modules[mod]['clippy_top'] = sorted(lints.items(), key=lambda kv: -kv[1])[:5]
            add(clippy_by_lint, lints)

    total = {}
    for mod, m in modules.items():
        m['signals'].setdefault('clippy_scored', 0)
        m['signals'].setdefault('clippy_all', 0)
        m['score'], m['categories'] = score_signals(m['signals'], have_clippy)
        m['debt'] = round((100 - m['score']) * m['signals']['loc'] / 1000, 1)
        add(total, m['signals'])
    overall, cats = score_signals(total, have_clippy)

    by_group = {}
    for lint, n in clippy_by_lint.items():
        g = clippy_groups.get(lint, 'unknown')
        by_group[g] = by_group.get(g, 0) + n

    loc = total.get('loc', 1)
    headline = {
        'score': overall,
        'code_lines': loc,
        'extern_c_fns': total.get('extern_c_fn', 0),
        'unsafe_per_kloc': round(total.get('unsafe', 0) * 1000 / loc, 2),
        'int_code_fields': total.get('int_code_field', 0),
        'magic_int_match_arms': total.get('magic_int_arm', 0),
        'library_unwraps': total.get('unwrap', 0),
        'get_set_accessors': total.get('get_set_accessor', 0),
        'out_params': total.get('out_param', 0),
        'question_marks_per_kloc': round(total.get('question_mark', 0) * 1000 / loc, 2),
    }
    if have_clippy:
        headline['clippy_warnings_per_kloc'] = round(total.get('clippy_scored', 0) * 1000 / loc, 2)
    return {
        'schema': SCHEMA,
        'headline': headline,
        'categories': cats,
        'totals': total,
        'clippy': {'by_group': dict(sorted(by_group.items(), key=lambda kv: -kv[1])),
                   'top_lints': sorted(clippy_by_lint.items(), key=lambda kv: -kv[1])[:30],
                   'allow_attributes': allow_clippy} if have_clippy else None,
        'modules': dict(sorted(modules.items(), key=lambda kv: kv[1]['score'])),
    }


def fmt_delta(new, old, higher_is_better=True):
    if old is None:
        return ''
    d = new - old
    if abs(d) < 1e-9:
        return ' (=)'
    good = (d > 0) == higher_is_better
    return f' ({"+" if d > 0 else ""}{round(d, 2)}{" ✓" if good else ""})'


def to_markdown(rep, base=None, top=25):
    h = rep['headline']
    bh = (base or {}).get('headline', {})
    out = ['# Rust nativeness scorecard', '',
           f"**Score: {h['score']} / 100**{fmt_delta(h['score'], bh.get('score'))}  ",
           f"{h['code_lines']:,} non-test code lines in {len(rep['modules'])} modules. "
           'Higher is more idiomatic; see `docs/nativeness/README.md` for what each signal means.', '',
           '## Headline numbers', '', '| Signal | Value |', '|---|---|']
    lower_better = {'extern_c_fns', 'unsafe_per_kloc', 'int_code_fields', 'magic_int_match_arms',
                    'library_unwraps', 'get_set_accessors', 'out_params', 'clippy_warnings_per_kloc'}
    for k, v in h.items():
        if k == 'score':
            continue
        delta = '' if k == 'code_lines' else fmt_delta(v, bh.get(k), k not in lower_better)
        out.append(f'| {k.replace("_", " ")} | {v}{delta} |')
    out += ['', '## Categories', '', '| Category | Weight | Count (weighted) | Per KLOC | Score |', '|---|---|---|---|---|']
    bc = (base or {}).get('categories', {})
    for key, c in rep['categories'].items():
        old = bc.get(key, {}).get('score')
        out.append(f"| {c['label']} | {c['weight']} | {c['weighted_count']} | {c['per_kloc']} | {c['score']}{fmt_delta(c['score'], old)} |")
    bm = (base or {}).get('modules', {})
    skip = {'loc', 'fn_count', 'question_mark', 'clippy_all', 'lock_unwrap', 'big_module', 'clippy_scored'}

    def row(name, m, extra):
        weakest = sorted(m['categories'].items(), key=lambda kv: kv[1]['score'])[:3]
        weak = ', '.join(f"{k} {c['score']:.0f}" for k, c in weakest)
        raw = sorted(((k, v) for k, v in m['signals'].items() if k not in skip and v), key=lambda kv: -kv[1])[:4]
        raws = ', '.join(f'{k} {v}' for k, v in raw)
        old = bm.get(name, {}).get('score')
        return f"| `{name}` | {m['score']}{fmt_delta(m['score'], old)} | {m['signals']['loc']:,} | {extra}{weak} | {raws} |"

    total_debt = sum(m['debt'] for m in rep['modules'].values()) or 1
    by_debt = sorted(rep['modules'].items(), key=lambda kv: -kv[1]['debt'])[:top]
    out += ['', f'## Where the work pays off (top {top} by debt)', '',
            'Debt = (100 - module score) x KLOC, so a big C-shaped module outranks a tiny one. '
            'Fixing the top of this list moves the overall score most.', '',
            '| Module | Score | Lines | Debt share | Weakest categories | Biggest raw signals |',
            '|---|---|---|---|---|---|']
    for name, m in by_debt:
        out.append(row(name, m, f"{100 * m['debt'] / total_debt:.1f}% | "))
    small = [(n, m) for n, m in rep['modules'].items() if m['signals']['loc'] >= MIN_RANKED_LOC][:top]
    out += ['', f'## Lowest scores (modules of {MIN_RANKED_LOC}+ lines)', '',
            '| Module | Score | Lines | Weakest categories | Biggest raw signals |', '|---|---|---|---|---|']
    for name, m in small:
        out.append(row(name, m, ''))
    if rep.get('clippy'):
        out += ['', '## Clippy', '', 'By group: ' + ', '.join(f'{g} {n:,}' for g, n in rep['clippy']['by_group'].items()), '',
                '| Lint | Count | Scored |', '|---|---|---|']
        for lint, n in rep['clippy']['top_lints'][:20]:
            out.append(f"| `{lint}` | {n:,} | {'no' if lint in CLIPPY_IGNORED else 'yes'} |")
    return '\n'.join(out) + '\n'


def dump_json(rep):
    """Compact, diff-friendly JSON: one line per module, zero signals dropped."""
    slim = dict(rep)
    slim['modules'] = {
        name: {'score': m['score'], 'debt': m['debt'], 'files': m['files'],
               'categories': {k: c['score'] for k, c in m['categories'].items()},
               'signals': {k: v for k, v in m['signals'].items() if v},
               **({'clippy_top': m['clippy_top']} if m.get('clippy_top') else {})}
        for name, m in rep['modules'].items()}
    head = {k: v for k, v in slim.items() if k != 'modules'}
    lines = ['{']
    for k, v in head.items():
        lines.append(f' {json.dumps(k)}: {json.dumps(v)},')
    lines.append(' "modules": {')
    mods = list(slim['modules'].items())
    for i, (name, m) in enumerate(mods):
        lines.append(f'  {json.dumps(name)}: {json.dumps(m)}' + (',' if i < len(mods) - 1 else ''))
    lines += [' }', '}']
    return '\n'.join(lines) + '\n'


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--src', default=None, help='crate src dir (default: rust/src next to this script)')
    ap.add_argument('--run-clippy', action='store_true', help='run cargo clippy with pedantic+nursery')
    ap.add_argument('--clippy-json', help='file with `cargo clippy --message-format=json` output')
    ap.add_argument('--json', help='write the full report as JSON here')
    ap.add_argument('--markdown', help='write the markdown report here (default: stdout)')
    ap.add_argument('--compare', help='earlier JSON report to show deltas against')
    ap.add_argument('--top', type=int, default=25, help='modules to list in the markdown')
    ap.add_argument('--fail-under', type=float, help='exit 1 if the overall score is below this')
    ap.add_argument('--fail-on-drop', type=float, help='exit 1 if the score fell more than this vs --compare')
    args = ap.parse_args(argv)

    repo = Path(__file__).resolve().parents[2]
    src = Path(args.src) if args.src else repo / 'rust' / 'src'
    clippy_lines = None
    if args.clippy_json:
        clippy_lines = Path(args.clippy_json).read_text(encoding='utf-8', errors='replace').splitlines()
    elif args.run_clippy:
        clippy_lines = run_clippy(src.parent)

    rep = build(src, clippy_lines)
    base = json.loads(Path(args.compare).read_text()) if args.compare else None
    if base is not None and (base.get('clippy') is None) != (rep.get('clippy') is None):
        print('note: one report has clippy data and the other does not; scores are not comparable, '
              'ignoring --compare', file=sys.stderr)
        if args.fail_on_drop is not None:
            raise SystemExit('--fail-on-drop needs both reports made the same way (with or without clippy)')
        base = None
    md = to_markdown(rep, base, args.top)
    if args.json:
        Path(args.json).write_text(dump_json(rep))
    if args.markdown:
        Path(args.markdown).write_text(md)
    else:
        sys.stdout.write(md)

    score = rep['headline']['score']
    if args.fail_under is not None and score < args.fail_under:
        print(f'score {score} is below {args.fail_under}', file=sys.stderr)
        return 1
    if args.fail_on_drop is not None and base is not None:
        drop = base['headline']['score'] - score
        if drop > args.fail_on_drop:
            print(f'score dropped by {round(drop, 2)} (limit {args.fail_on_drop})', file=sys.stderr)
            return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
