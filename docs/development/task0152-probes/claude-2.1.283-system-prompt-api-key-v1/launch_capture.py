import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys

DIRECTORY = Path(__file__).resolve().parent
SOURCE = DIRECTORY.parent / 'capture_profile.py'
EXPECTED_SOURCE = 'c51fc46af7ed65f1e6a744f6174bfa3743ad5fc4f87838ed2a0f484c909f82b7'
EXPECTED_EXECUTABLE = 'd8cb1e5c79684cc12a8bfc813e3a2073406921b6245744b3009be3ab5651d21e'

def main():
    assert sys.flags.isolated and len(sys.argv) == 2
    assert hashlib.sha256(SOURCE.read_bytes()).hexdigest() == EXPECTED_SOURCE
    executable = Path(sys.argv[1]).resolve(strict=True)
    assert hashlib.sha256(executable.read_bytes()).hexdigest() == EXPECTED_EXECUTABLE
    spec = importlib.util.spec_from_file_location('capture', SOURCE)
    capture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(capture)
    capture.PROFILE_ID = DIRECTORY.name
    capture.ARGV = capture.ARGV[:-1] + ['--system-prompt', 'role', capture.PROMPT]
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        exit_code = capture.main()
    summary = json.loads(output.getvalue())
    root = Path(summary['artifact_directory'])
    manifest_path = root / 'run.json'
    manifest = json.loads(manifest_path.read_text())
    manifest['launch_wrapper_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    manifest['launch_wrapper_identity'] = str(Path(__file__).relative_to(DIRECTORY.parent))
    manifest['launch_hypothesis_sha256'] = hashlib.sha256((DIRECTORY / 'HYPOTHESIS.md').read_bytes()).hexdigest()
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps(summary))
    return exit_code

if __name__ == '__main__':
    sys.exit(main())
