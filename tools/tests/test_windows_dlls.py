import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('runtime', Path(__file__).parents[1] / 'build-test-runtime.py')
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)


class WindowsDllFixtures(unittest.TestCase):
    def test_transitive_toolchain_libraries_are_copied_next_to_ffmpeg(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            runtime_dir = root / 'runtime'
            toolchain = root / 'toolchain'
            runtime_dir.mkdir()
            toolchain.mkdir()
            (runtime_dir / 'avutil-59.dll').write_bytes(b'ffmpeg')
            (toolchain / 'libgcc_s_seh-1.dll').write_bytes(b'gcc')
            (toolchain / 'libwinpthread-1.dll').write_bytes(b'thread')
            imports = {
                'avutil-59.dll': 'DLL Name: libgcc_s_seh-1.dll\nDLL Name: KERNEL32.dll',
                'libgcc_s_seh-1.dll': 'DLL Name: libwinpthread-1.dll',
                'libwinpthread-1.dll': 'DLL Name: KERNEL32.dll',
            }
            with patch.object(runtime.subprocess, 'check_output', side_effect=lambda args, **kw: imports[Path(args[-1]).name]):
                runtime.copy_toolchain_dlls(runtime_dir, toolchain)
            self.assertEqual((runtime_dir / 'libgcc_s_seh-1.dll').read_bytes(), b'gcc')
            self.assertEqual((runtime_dir / 'libwinpthread-1.dll').read_bytes(), b'thread')
            self.assertFalse((runtime_dir / 'KERNEL32.dll').exists())

    def test_msys_shell_dependency_is_rejected(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            (root / 'avutil-59.dll').touch()
            with patch.object(runtime.subprocess, 'check_output', return_value='DLL Name: msys-2.0.dll'):
                with self.assertRaisesRegex(RuntimeError, 'MSYS shell'):
                    runtime.copy_toolchain_dlls(root, root)
