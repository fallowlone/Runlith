import copy
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys

DIRECTORY = Path(__file__).resolve().parent

def main():
    spec = importlib.util.spec_from_file_location('checker', DIRECTORY.parent / 'profile_check.py')
    checker = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(checker)
    manifest = checker.strict_json((DIRECTORY / 'run.json').read_text())
    for name, key in [('launch_capture.py', 'launch_wrapper_sha256'),
                      ('HYPOTHESIS.md', 'launch_hypothesis_sha256')]:
        assert hashlib.sha256((DIRECTORY / name).read_bytes()).hexdigest() == manifest[key]
    assert manifest['argv'] == ['-p', '--safe-mode', '--tools', '', '--disallowedTools',
                                'mcp__*', '--output-format', 'json', '--system-prompt',
                                'role', 'Reply with exactly TASK0152_PROFILE_OK and nothing else.']
    rows = [checker.strict_json(line) for line in (DIRECTORY / 'sanitized-capture.jsonl').read_text().splitlines()]
    derived = checker.strict_json((DIRECTORY / 'normalized.json').read_text())
    assert checker.normalize(rows) == derived['requests']
    for name, mutate in [('sequence_widening', lambda r: r.append(copy.deepcopy(r[0]))),
                         ('scope_widening', lambda r: checker.mutate_body(r, lambda b: b.update(scope='*')))]:
        changed = copy.deepcopy(rows)
        mutate(changed)
        assert checker.inspect(changed, derived['requests']) == ['profile_mismatch'], name
    checker.DIRECTORY = DIRECTORY
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        exit_code = checker.main()
    result = json.loads(output.getvalue())
    result['checks_passed'].extend(['launch_wrapper_and_hypothesis_hashes', 'exact_launch_argv',
                                  'sequence_widening', 'scope_widening'])
    print(json.dumps(result, indent=2))
    return exit_code

if __name__ == '__main__':
    sys.exit(main())
