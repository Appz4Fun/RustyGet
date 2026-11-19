"""Unit tests for the nativeness scorecard's lexer, signals and scoring."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import score  # noqa: E402


def scan(src):
    return score.scan_file(src)[0]


class Lexer(unittest.TestCase):
    def test_comments_and_strings_are_blanked(self):
        src = 'let a = "x.unwrap()"; // y.unwrap()\n/* z.unwrap() /* nested */ */ let b = r#"q.unwrap()"#;\n'
        out = score.blank_comments_and_strings(src)
        self.assertNotIn('unwrap', out)
        self.assertEqual(out.count('\n'), src.count('\n'))
        self.assertIn('let b', out)

    def test_lifetimes_survive_and_chars_blank(self):
        out = score.blank_comments_and_strings("fn f<'a>(x: &'a str) -> char { '{' }")
        self.assertIn("<'a>", out)
        self.assertNotIn("'{'", out)

    def test_cfg_test_on_field_variant_and_arm_blanks_only_that_element(self):
        src = ('struct S { a: u8, #[cfg(test)] b: u8 }\n'
               'enum E { A, #[cfg(test)] B(i32), C }\n'
               'fn g(x: u8) { match x { 0 => {}\n #[cfg(test)]\n 1 => { t.unwrap(); }\n _ => {} } }\n'
               'impl S { fn f() { x.unwrap(); } }\n')
        s = scan(src)
        self.assertEqual(s['unwrap'], 1)
        self.assertEqual(s['fn_count'], 2)

    def test_cfg_all_with_test_anywhere_is_ignored(self):
        src = ('#[cfg(all(unix, test))]\nfn t() { a.unwrap(); }\n'
               '#[cfg(not(test))]\nfn real() { b.unwrap(); }\n'
               '#[cfg(all(unix, not(test)))]\nfn real2() { c.unwrap(); }\n'
               '#[cfg(all(any(unix, test), x))]\nfn real3() { d.unwrap(); }\n')
        self.assertEqual(scan(src)['unwrap'], 3)

    def test_cfg_test_arm_with_else_is_fully_blanked(self):
        src = 'fn f(x: u8) { match x {\n #[cfg(test)]\n 1 => if a { b } else { c.unwrap() },\n _ => 0 } }\n'
        self.assertEqual(scan(src)['unwrap'], 0)

    def test_cfg_test_module_is_ignored(self):
        src = 'fn a() { x.unwrap(); }\n#[cfg(test)]\nmod tests {\n  fn b() { y.unwrap(); z.unwrap(); }\n}\n'
        self.assertEqual(scan(src)['unwrap'], 1)


class Signals(unittest.TestCase):
    def test_ffi(self):
        s = scan('#[unsafe(no_mangle)]\npub unsafe extern "C" fn f(p: *mut Foo, n: c_int) -> c_int { 0 }\n')
        self.assertEqual(s['extern_c_fn'], 1)
        self.assertEqual(s['no_mangle'], 1)
        self.assertEqual(s['raw_ptr'], 1)
        self.assertEqual(s['c_types'], 2)
        self.assertEqual(s['unsafe'], 1)

    def test_integer_codes(self):
        src = ('struct S { pub status: i32, pub error_kind: u8, pub count: i32 }\n'
               'fn f(op: c_int) -> i32 { match op { 0 => 1, 1 | 2 => 2, _ => return -1 } }\n')
        s = scan(src)
        self.assertEqual(s['int_code_field'], 3)  # status, error_kind, op; not count
        self.assertEqual(s['sentinel_minus_one'], 1)

    def test_magic_arms_one_per_line(self):
        src = 'fn f(x: u8) {\n match x {\n  0 => a(),\n  1 | 2 => b(),\n  3..=9 => c(),\n  _ => d(),\n }\n}\n'
        self.assertEqual(scan(src)['magic_int_arm'], 3)

    def test_api_shape(self):
        src = ('impl T {\n fn get_name(&self) -> &str { &self.n }\n fn set_name(&mut self, n: String) {}\n'
               ' fn load(&mut self, out: &mut String, ok: &mut bool) -> bool { true }\n'
               ' fn is_ready(&self, x: u8) -> bool { true }\n}\n')
        s = scan(src)
        self.assertEqual(s['get_set_accessor'], 2)
        self.assertEqual(s['out_param'], 2)
        self.assertEqual(s['bool_status_fn'], 1)  # load, not is_ready

    def test_panics_skip_lock_unwrap(self):
        s = scan('fn f() { m.lock().unwrap(); x.unwrap(); y.expect("e"); unreachable!() }')
        self.assertEqual(s['unwrap'], 1)
        self.assertEqual(s['lock_unwrap'], 1)
        self.assertEqual(s['expect'], 1)
        self.assertEqual(s['panic_macro'], 1)

    def test_loops_casts_globals(self):
        src = ('static COUNT: AtomicUsize = AtomicUsize::new(0);\nstatic mut G: i32 = 0;\n'
               'fn f(v: &[u8]) { for i in 0..v.len() { let _ = v[i] as u32; } }\n')
        s = scan(src)
        self.assertEqual(s['index_loop'], 1)
        self.assertEqual(s['as_cast'], 1)
        self.assertEqual(s['static_shared'], 1)
        self.assertEqual(s['static_mut'], 1)

    def test_fn_with_closure_bound_generics_is_found(self):
        fns = score.analyze_functions('fn with<R, F: FnOnce() -> R>(a: u8, f: F) -> R { f() }')
        self.assertEqual([f['name'] for f in fns], ['with'])
        self.assertEqual(len(fns[0]['params']), 2)

    def test_stringy_result_respects_nesting(self):
        self.assertEqual(scan('fn a() -> Result<HashMap<u32, String>, io::Error> { todo() }')['result_stringy_err'], 0)
        self.assertEqual(scan('fn b() -> Result<Vec<u8>, String> { todo() }')['result_stringy_err'], 1)

    def test_extern_c_unwind(self):
        self.assertEqual(scan('pub extern "C-unwind" fn f() {}')['extern_c_fn'], 1)

    def test_long_fn_and_many_params(self):
        body = '\n'.join(f'  let x{i} = {i};' for i in range(score.LONG_FN + 5))
        src = f'fn big(a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8) {{\n{body}\n}}\n'
        s = scan(src)
        self.assertEqual(s['long_fn'], 1)
        self.assertEqual(s['many_params'], 1)


class Scoring(unittest.TestCase):
    def test_clean_code_scores_100(self):
        sig = {'loc': 1000}
        overall, cats = score.score_signals(sig, have_clippy=False)
        self.assertEqual(overall, 100.0)
        self.assertNotIn('clippy', cats)

    def test_half_density_scores_50(self):
        self.assertAlmostEqual(score.sub_score(4.0, 4.0), 50.0)

    def test_more_problems_lower_score(self):
        a, _ = score.score_signals({'loc': 1000, 'unwrap': 5}, False)
        b, _ = score.score_signals({'loc': 1000, 'unwrap': 50}, False)
        self.assertGreater(a, b)
        self.assertGreater(b, 0)


class EndToEnd(unittest.TestCase):
    def test_report_on_a_tiny_crate(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / 'src'
            (src / 'bad').mkdir(parents=True)
            (src / 'good.rs').write_text('pub fn add(a: u32, b: u32) -> Option<u32> { a.checked_add(b) }\n')
            (src / 'bad.rs').write_text('pub mod inner;\n')
            (src / 'bad' / 'inner.rs').write_text(
                'pub extern "C" fn f(p: *mut u8) -> i32 { unsafe { *p = 0 }; x.unwrap(); -1 }\n')
            (src / 'bad' / 'tests.rs').write_text('fn t() { a.unwrap(); }\n')
            out_json = Path(tmp) / 'r.json'
            out_md = Path(tmp) / 'r.md'
            rc = score.main(['--src', str(src), '--json', str(out_json), '--markdown', str(out_md)])
            self.assertEqual(rc, 0)
            rep = json.loads(out_json.read_text())
            self.assertEqual(set(rep['modules']), {'good', 'bad'})
            self.assertEqual(rep['modules']['good']['score'], 100.0)
            self.assertLess(rep['modules']['bad']['score'], 75)
            self.assertEqual(rep['totals']['unwrap'], 1)  # tests.rs skipped
            self.assertIn('Rust nativeness scorecard', out_md.read_text())
            rc = score.main(['--src', str(src), '--markdown', str(out_md), '--compare', str(out_json),
                             '--fail-under', '99'])
            self.assertEqual(rc, 1)

    def test_clippy_json_is_attributed_to_modules(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / 'src'
            src.mkdir()
            (src / 'a.rs').write_text('pub fn f() {}\n')
            msg = {'reason': 'compiler-message', 'message': {
                'code': {'code': 'clippy::cast_lossless'}, 'level': 'warning',
                'children': [{'message': '`-W clippy::cast-lossless` implied by `-W clippy::pedantic`'}],
                'spans': [{'is_primary': True, 'file_name': 'src/a.rs'}]}}
            cj = Path(tmp) / 'c.json'
            done = {'reason': 'build-finished', 'success': True}
            cj.write_text(json.dumps(msg) + '\n' + json.dumps(done) + '\n')
            rep = score.build(src, cj.read_text().splitlines())
            self.assertEqual(rep['modules']['a']['signals']['clippy_scored'], 1)
            self.assertEqual(rep['clippy']['by_group'], {'pedantic': 1})

    def test_failed_clippy_run_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / 'src'
            src.mkdir()
            (src / 'a.rs').write_text('pub fn f() {}\n')
            for lines in ([], [json.dumps({'reason': 'build-finished', 'success': False})]):
                with self.assertRaises(SystemExit):
                    score.build(src, lines)

    def test_test_gated_external_modules_are_skipped(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / 'src'
            (src / 'outer').mkdir(parents=True)
            (src / 'lib.rs').write_text('pub mod outer;\n#[cfg(test)]\nmod probe;\n')
            (src / 'probe.rs').write_text('fn p() { x.unwrap(); }\n')
            (src / 'outer.rs').write_text('#[cfg(test)]\nmod fixtures;\npub fn f() {}\n')
            (src / 'outer' / 'fixtures.rs').write_text('fn q() { y.unwrap(); }\n')
            rep = score.build(src, None)
            self.assertNotIn('probe', rep['modules'])
            self.assertEqual(rep['totals'].get('unwrap', 0), 0)


if __name__ == '__main__':
    unittest.main()
