from __future__ import annotations

import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Optional, Sequence


@dataclass
class RunResult:
    returncode: int
    stdout: str
    stderr: str

    @property
    def ok(self) -> bool:
        return self.returncode == 0


class CliRunner:
    def __init__(self, cwd: Optional[Path] = None, env: Optional[dict[str, str]] = None, timeout: int = 30) -> None:
        self.cwd = cwd or Path.cwd()
        self.env = env
        self.timeout = timeout

    def run(self, args: Sequence[str], check: bool = False) -> RunResult:
        proc = subprocess.run(
            list(args),
            capture_output=True,
            text=True,
            cwd=str(self.cwd),
            env=self.env,
            timeout=self.timeout,
        )
        result = RunResult(returncode=proc.returncode, stdout=proc.stdout, stderr=proc.stderr)
        if check and not result.ok:
            raise subprocess.CalledProcessError(proc.returncode, args, proc.stdout, proc.stderr)
        return result

    def run_cargo(self, *args: str) -> RunResult:
        return self.run(["cargo", *args])

    def run_python(self, script: Path, *args: str) -> RunResult:
        return self.run([sys.executable, str(script), *args])

    def which(self, binary: str) -> Optional[Path]:
        result = self.run(["which", binary])
        if result.ok and result.stdout.strip():
            return Path(result.stdout.strip())
        return None
