import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('publish', Path(__file__).parents[1] / 'publish.py')
publish = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publish)
ENTRY = {'name': 'valle-ffmpeg-sys', 'vers': '0.1.0', 'cksum': 'expected', 'yanked': False}
ARGS = ('valle-ffmpeg-sys', '0.1.0', 'expected')


class PublishRecovery(unittest.TestCase):
    def test_existing_identical_package_is_not_uploaded_again(self):
        with patch.object(publish, 'index_version', return_value=ENTRY), patch.object(publish.subprocess, 'run') as run:
            publish.publish_package(*ARGS, upload=True)
            run.assert_not_called()

    def test_existing_different_or_yanked_package_stops_release(self):
        for entry in (dict(ENTRY, cksum='different'), dict(ENTRY, yanked=True)):
            with patch.object(publish, 'index_version', return_value=entry), patch.object(publish.subprocess, 'run') as run:
                with self.assertRaises(RuntimeError):
                    publish.publish_package(*ARGS, upload=True)
                run.assert_not_called()

    def test_dry_run_never_uploads(self):
        with patch.object(publish, 'index_version', return_value=None), patch.object(publish.subprocess, 'run') as run:
            publish.publish_package(*ARGS, upload=False)
            run.assert_not_called()

    def test_cargo_timeout_after_upload_resumes_when_index_catches_up(self):
        with patch.object(publish, 'index_version', side_effect=[None, None, ENTRY]), \
                patch.object(publish.subprocess, 'run', return_value=subprocess.CompletedProcess([], 101)) as run, \
                patch.object(publish.time, 'sleep'):
            publish.publish_package(*ARGS, upload=True)
            self.assertEqual(run.call_count, 1)

    def test_index_timeout_does_not_trigger_another_upload(self):
        with patch.object(publish, 'index_version', return_value=None):
            with self.assertRaisesRegex(RuntimeError, 'same revision'):
                publish.wait_for_index(*ARGS, timeout=0)

    def test_network_failure_is_not_treated_as_unpublished(self):
        with patch.object(publish, 'index_version', side_effect=TimeoutError), patch.object(publish.subprocess, 'run') as run:
            with self.assertRaises(TimeoutError):
                publish.publish_package(*ARGS, upload=True)
            run.assert_not_called()
