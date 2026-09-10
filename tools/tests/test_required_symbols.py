import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('symbols', Path(__file__).parents[1] / 'check-required-symbols.py')
symbols = importlib.util.module_from_spec(spec)
spec.loader.exec_module(symbols)


class SymbolCoverage(unittest.TestCase):
    def test_import_alias_and_macro_reference_are_not_lost(self):
        source = '''use ffi::av_frame_alloc as allocate;
        invoke!(ffi::av_frame_free, &mut frame);
        // av_comment
        /* av_comment /* nested */ av_comment */
        let a = r###"av_raw /* text */"###;
        let b = "av_string \\" more";
        let c = '/';
        let d: &'static str;
        '''
        names = symbols.identifiers(source)
        self.assertEqual({n for n in names if n.startswith('av_')}, {'av_frame_alloc', 'av_frame_free'})
        with self.assertRaisesRegex(ValueError, 'av_frame_free'):
            symbols.audit({'av_frame_alloc'}, {'av_frame_alloc', 'av_frame_free'}, names)
        self.assertEqual(symbols.audit({'av_frame_alloc', 'av_frame_free'}, {'av_frame_alloc', 'av_frame_free'}, names), 2)

    def test_rust_helpers_are_not_native_symbols(self):
        self.assertEqual(symbols.audit({'av_frame_alloc'}, {'av_frame_alloc'}, {'AVERROR', 'av_q2d', 'av_frame_alloc'}), 1)
