import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

EXECUTABLE = Path("/Users/artemmac/.local/bin/codex")
OUTPUT = Path(__file__).resolve().parent / "cli.json"
COMMANDS = (("--version",), ("--help",), ("exec", "--help"), ("login", "--help"))


def capture():
    resolved = EXECUTABLE.resolve(strict=True)
    identity = hashlib.sha256(resolved.read_bytes()).hexdigest()
    records = []
    with tempfile.TemporaryDirectory(prefix="runlith-task0156-help-") as scratch:
        environment = {
            "HOME": "/var/empty",
            "CODEX_HOME": "/var/empty",
            "PATH": "/usr/bin:/bin",
            "TMPDIR": scratch,
            "LANG": "C",
        }
        for arguments in COMMANDS:
            started = time.monotonic()
            result = subprocess.run(
                [str(EXECUTABLE), *arguments],
                env=environment,
                cwd=scratch,
                stdin=subprocess.DEVNULL,
                capture_output=True,
                timeout=5,
                check=False,
            )
            assert len(result.stdout) + len(result.stderr) <= 128 * 1024
            records.append({
                "argv": [str(EXECUTABLE), *arguments],
                "cwd": "<owned-empty-temp>",
                "environment": {**environment, "TMPDIR": "<owned-empty-temp>"},
                "stdin": "closed",
                "timeoutSeconds": 5,
                "captureCeilingBytes": 131072,
                "elapsedSeconds": round(time.monotonic() - started, 3),
                "exitCode": result.returncode,
                "stdout": result.stdout.decode("utf-8", errors="strict"),
                "stderr": result.stderr.decode("utf-8", errors="strict").replace(scratch, "<owned-empty-temp>"),
                "stdoutSha256": hashlib.sha256(result.stdout).hexdigest(),
                "stderrSha256": hashlib.sha256(result.stderr).hexdigest(),
            })
        assert records[0]["stdout"].strip() == "codex-cli 0.159.2"
        assert all(record["exitCode"] == 0 for record in records)
    assert not os.path.exists(scratch)
    OUTPUT.write_text(json.dumps({
        "observedAtUtc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "executable": str(EXECUTABLE),
        "resolvedExecutable": str(resolved),
        "executableSha256": identity,
        "isolation": "Empty inherited environment; HOME/CODEX_HOME=/var/empty; owned empty cwd/temp. No auth status, login/logout or inference. Help/version are the only CLI actions. No OS confinement compatibility claim.",
        "captureNote": "Fixed help/version output checked against 128 KiB after capture; not a production streaming output limiter. Raw stream hashes precede temp-path normalization.",
        "records": records,
        "cleanup": "TemporaryDirectory removed and absence asserted; all fixed CLI commands exited and were reaped.",
    }, indent=2) + "\n")
    print(f"PASS: {len(records)} fixed version/help commands; version 0.159.2; temporary directory removed")


if __name__ == "__main__":
    capture()
