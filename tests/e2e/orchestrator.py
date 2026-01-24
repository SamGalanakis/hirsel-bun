"""Orchestrator setup and management for E2E tests."""

import json
import os
import secrets
import shutil
import subprocess
import tempfile
import time
from dataclasses import dataclass, field
from enum import Enum, auto
from pathlib import Path

import pytest

# Use /var/tmp for test temp dirs - persists across reboots and avoids
# tmpfs memory limits and user quota issues on /tmp
TEMP_DIR = "/var/tmp"


class OrchestratorMode(Enum):
    """Mode of orchestrator operation."""
    LOCAL = auto()   # Run hirsel directly on local machine
    REMOTE = auto()  # Connect to existing remote coordinator


@dataclass
class Orchestrator:
    """Manages the orchestrator (hirsel server) for tests.

    Profiles:
    - "local": Run hirsel directly on local machine
    - "<name>": Use profile from config.toml (e.g., remote Fly coordinator)
    """

    profile: str  # Profile name or "local"
    binary: str  # Path to hirsel binary
    worker_scale: int = 1  # Number of workers per run
    hirsel_dir: str | None = None  # Override ~/.hirsel (None = create temp dir)

    # Generated at setup time
    run_id: str = field(default_factory=lambda: secrets.token_hex(4))
    api_key: str = field(default_factory=lambda: secrets.token_hex(32))
    work_dir: str | None = None
    config_dir: str | None = None
    host: str = "localhost"
    port: int = 8080
    remote_url: str | None = None  # For remote profiles

    _temp_dirs: list[str] = field(default_factory=list)
    _mode: OrchestratorMode = field(default=OrchestratorMode.LOCAL)
    _owns_hirsel_dir: bool = field(default=False)  # True if we created temp hirsel dir

    def setup(self) -> None:
        """Set up the orchestrator environment."""
        self._resolve_profile()

        match self._mode:
            case OrchestratorMode.LOCAL:
                self._setup_local()
            case OrchestratorMode.REMOTE:
                self._setup_remote()

    def _resolve_profile(self) -> None:
        """Resolve profile name to mode and settings."""
        match self.profile:
            case "local":
                self._mode = OrchestratorMode.LOCAL
            case _:
                # Load profile from config file
                profile_config = self._load_profile_config(self.profile)
                if profile_config:
                    mode_str = profile_config.get("mode", "local")
                    match mode_str:
                        case "local":
                            self._mode = OrchestratorMode.LOCAL
                        case "remote":
                            self._mode = OrchestratorMode.REMOTE
                            self.remote_url = profile_config.get("url")
                            self.api_key = profile_config.get("api_key", self.api_key)
                        case _:
                            self._mode = OrchestratorMode.LOCAL
                else:
                    # Assume local if profile not found
                    self._mode = OrchestratorMode.LOCAL

    def _load_profile_config(self, profile_name: str) -> dict | None:
        """Load profile configuration from config file."""
        # Check provided hirsel_dir first, then fallback to ~/.hirsel
        config_paths = []
        if self.hirsel_dir:
            config_paths.append(Path(self.hirsel_dir) / "config.toml")
        config_paths.append(Path.home() / ".hirsel" / "config.toml")

        for config_path in config_paths:
            if not config_path.exists():
                continue
            try:
                import tomllib
                with open(config_path, "rb") as f:
                    config = tomllib.load(f)
                profile = config.get("profiles", {}).get(profile_name)
                if profile:
                    return profile
            except Exception:
                continue
        return None

    def teardown(self) -> None:
        """Clean up the orchestrator environment."""
        self._teardown_local()

    def _setup_remote(self) -> None:
        """Set up for remote orchestrator (already running)."""
        if not self.remote_url:
            pytest.skip(f"Profile {self.profile} has no remote URL configured")

        # Create local temp work directory for specs
        self.work_dir = tempfile.mkdtemp(prefix="hirsel-e2e-", dir=TEMP_DIR)
        self._temp_dirs.append(self.work_dir)

        # Initialize git repo
        subprocess.run(["git", "init"], cwd=self.work_dir, capture_output=True)
        subprocess.run(
            ["git", "config", "user.email", "test@test.com"],
            cwd=self.work_dir,
            capture_output=True,
        )
        subprocess.run(
            ["git", "config", "user.name", "Test"],
            cwd=self.work_dir,
            capture_output=True,
        )

        readme_path = Path(self.work_dir) / "README.md"
        readme_path.write_text("# Test Project\n")
        subprocess.run(["git", "add", "."], cwd=self.work_dir, capture_output=True)
        subprocess.run(
            ["git", "commit", "-m", "initial"],
            cwd=self.work_dir,
            capture_output=True,
        )

        # Set up hirsel config directory
        if self.hirsel_dir:
            self.config_dir = self.hirsel_dir
            Path(self.config_dir).mkdir(parents=True, exist_ok=True)
        else:
            # Create isolated temp directory for test
            self.config_dir = tempfile.mkdtemp(prefix="hirsel-config-", dir=TEMP_DIR)
            self._temp_dirs.append(self.config_dir)
            self._owns_hirsel_dir = True

        self.host = self.remote_url

    def _get_unique_daemon_port(self) -> int:
        """Get a unique daemon port for this test to avoid conflicts."""
        import socket

        # Find an available port
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.bind(("127.0.0.1", 0))
            return s.getsockname()[1]

    def _setup_local(self) -> None:
        """Set up local orchestrator."""
        # Get a unique port for this test's daemon
        self._daemon_port = self._get_unique_daemon_port()

        # Create temp work directory
        self.work_dir = tempfile.mkdtemp(prefix="hirsel-e2e-", dir=TEMP_DIR)
        self._temp_dirs.append(self.work_dir)

        # Initialize git repo
        subprocess.run(
            ["git", "init"],
            cwd=self.work_dir,
            capture_output=True,
            check=True,
        )
        subprocess.run(
            ["git", "config", "user.email", "test@test.com"],
            cwd=self.work_dir,
            capture_output=True,
        )
        subprocess.run(
            ["git", "config", "user.name", "Test"],
            cwd=self.work_dir,
            capture_output=True,
        )

        # Create initial commit
        readme_path = Path(self.work_dir) / "README.md"
        readme_path.write_text("# Test Project\n")
        subprocess.run(["git", "add", "."], cwd=self.work_dir, capture_output=True)
        subprocess.run(
            ["git", "commit", "-m", "initial"],
            cwd=self.work_dir,
            capture_output=True,
        )

        # Set up hirsel config directory
        if self.hirsel_dir:
            # Use provided directory
            self.config_dir = self.hirsel_dir
            Path(self.config_dir).mkdir(parents=True, exist_ok=True)
        else:
            # Create isolated temp directory for test
            self.config_dir = tempfile.mkdtemp(prefix="hirsel-config-", dir=TEMP_DIR)
            self._temp_dirs.append(self.config_dir)
            self._owns_hirsel_dir = True

    def _teardown_local(self) -> None:
        """Clean up local orchestrator."""
        # Clean up temp directories (includes hirsel_dir if we created it)
        for temp_dir in self._temp_dirs:
            shutil.rmtree(temp_dir, ignore_errors=True)

    def write_config(self, config: str) -> None:
        """Write configuration to orchestrator's config.toml.

        Note: This overwrites the entire config file. Each test should write
        the complete config it needs.
        """
        config_file = Path(self.config_dir) / "config.toml"
        config_file.parent.mkdir(parents=True, exist_ok=True)
        config_file.write_text(config)

    def setup_ssh_loopback(self) -> None:
        """Set up SSH keys for localhost loopback testing."""
        # For local/remote mode, assume SSH is already set up
        pass

    def copy_spec(self, scenario: str) -> tuple[str, str | None]:
        """Copy scenario spec, eval, and project files to work directory.

        Returns (spec_path, eval_path or None).
        """
        scenarios_dir = Path(__file__).parent.parent / "scenarios"
        scenario_dir = scenarios_dir / scenario
        spec_src = scenario_dir / "spec.md"
        eval_src = scenario_dir / "eval.md"
        project_dir = scenario_dir / "project"

        spec_content = spec_src.read_text()
        spec_path = f"{self.work_dir}/spec.md"

        eval_path = None
        eval_content = None
        if eval_src.exists():
            eval_content = eval_src.read_text()
            eval_path = f"{self.work_dir}/eval.md"

        # Collect project files to copy
        project_files: list[tuple[str, str]] = []
        if project_dir.is_dir():
            for file_path in project_dir.rglob("*"):
                if file_path.is_file():
                    rel_path = file_path.relative_to(project_dir)
                    project_files.append((str(rel_path), file_path.read_text()))

        Path(spec_path).write_text(spec_content)
        if eval_content:
            Path(eval_path).write_text(eval_content)
        # Copy project files
        for rel_path, content in project_files:
            dest = Path(self.work_dir) / rel_path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(content)

        return spec_path, eval_path

    def start_run(
        self,
        name: str,
        spec: str | Path,
        eval_spec: str | Path | None = None,
        runner: str = "local",
        extra_args: list[str] | None = None,
    ) -> None:
        """Start a hirsel run."""
        cmd_parts = [
            self.binary,
            "go",
            name,
            str(spec),
            "--project",
            self.work_dir,
            "--yolo",
            "--runner",
            runner,
            "--workers",
            str(self.worker_scale),
        ]

        # Add profile for remote mode
        if self._mode == OrchestratorMode.REMOTE:
            cmd_parts.extend(["--profile", self.profile])

        if eval_spec:
            cmd_parts.extend(["--eval", str(eval_spec)])

        if extra_args:
            cmd_parts.extend(extra_args)

        # Add credentials
        env_vars = self._get_env()

        env = os.environ.copy()
        env.update(env_vars)
        subprocess.run(
            cmd_parts,
            cwd=self.work_dir,
            env=env,
            capture_output=True,
        )

    def _get_env(self) -> dict[str, str]:
        """Get environment variables for hirsel commands."""
        env = {}

        # Set hirsel root to our isolated directory
        if self.config_dir:
            env["HIRSEL_ROOT"] = self.config_dir

        # Set daemon port to avoid conflicts with other tests/daemons
        if hasattr(self, "_daemon_port") and self._daemon_port:
            env["HIRSEL_DAEMON_PORT"] = str(self._daemon_port)

        # Claude OAuth credentials
        creds_file = Path.home() / ".claude" / ".credentials.json"
        if creds_file.exists():
            try:
                creds = json.loads(creds_file.read_text())
                if token := creds.get("access_token"):
                    env["CLAUDE_ACCESS_TOKEN"] = token
            except (json.JSONDecodeError, KeyError):
                pass

        # Anthropic API key
        if api_key := os.environ.get("ANTHROPIC_API_KEY"):
            env["ANTHROPIC_API_KEY"] = api_key

        # Fly token
        if token := os.environ.get("FLY_API_TOKEN"):
            env["FLY_API_TOKEN"] = token

        return env

    def get_run(self, name: str) -> dict:
        """Get run information as JSON."""
        cmd = [self.binary, "view", name, "--json"]
        if self._mode == OrchestratorMode.REMOTE:
            cmd.extend(["--profile", self.profile])
        env = os.environ.copy()
        env.update(self._get_env())
        result = subprocess.run(cmd, capture_output=True, text=True, env=env)
        if result.returncode == 0:
            try:
                return json.loads(result.stdout)
            except json.JSONDecodeError:
                return {}
        return {}

    def get_status(self, name: str) -> str:
        """Get current run status."""
        run_info = self.get_run(name)
        return run_info.get("status", "unknown")

    def wait_for_run(self, name: str, timeout: int = 300) -> str:
        """Wait for run to complete. Returns final status."""
        start_time = time.time()

        while True:
            elapsed = time.time() - start_time
            if elapsed >= timeout:
                pytest.fail(f"Run {name} timed out after {timeout}s")

            status = self.get_status(name)

            if status in ("completed", "delivered", "pass", "fail", "done"):
                return status
            if status == "error":
                pytest.fail(f"Run {name} failed with error")

            time.sleep(5)

    def wait_for_status(self, name: str, expected: str, timeout: int = 60) -> None:
        """Wait for run to reach a specific status."""
        start_time = time.time()

        while True:
            elapsed = time.time() - start_time
            if elapsed >= timeout:
                pytest.fail(
                    f"Run {name} did not reach status {expected} "
                    f"within {timeout}s (current: {self.get_status(name)})"
                )

            if self.get_status(name) == expected:
                return

            time.sleep(2)

    def wait_for_working_or_done(self, name: str, timeout: int = 60) -> bool:
        """Wait for run to start working. Returns False if run completed first."""
        done_statuses = ("completed", "delivered", "pass", "fail", "done")
        start_time = time.time()

        while True:
            elapsed = time.time() - start_time
            if elapsed >= timeout:
                return False

            status = self.get_status(name)

            if status == "working":
                return True
            if status in done_statuses:
                return False

            time.sleep(2)

    def wait_for_paused_or_done(self, name: str, timeout: int = 30) -> bool:
        """Wait for run to pause. Returns False if run completed first."""
        done_statuses = ("completed", "delivered", "pass", "fail", "done")
        start_time = time.time()

        while True:
            elapsed = time.time() - start_time
            if elapsed >= timeout:
                return False

            status = self.get_status(name)

            if status == "paused":
                return True
            if status in done_statuses:
                return False

            time.sleep(2)

    def pause_run(self, name: str) -> None:
        """Pause a running run."""
        cmd = [self.binary, "pause", name]
        if self._mode == OrchestratorMode.REMOTE:
            cmd.extend(["--profile", self.profile])
        env = os.environ.copy()
        env.update(self._get_env())
        subprocess.run(cmd, capture_output=True, env=env)

    def resume_run(self, name: str) -> None:
        """Resume a paused run."""
        cmd = [self.binary, "resume", name]
        if self._mode == OrchestratorMode.REMOTE:
            cmd.extend(["--profile", self.profile])
        env = os.environ.copy()
        env.update(self._get_env())
        subprocess.run(cmd, capture_output=True, env=env)

    def deliver_run(self, name: str) -> None:
        """Deliver a completed run (merge staging to workspace)."""
        cmd = [self.binary, "deliver", name]
        if self._mode == OrchestratorMode.REMOTE:
            cmd.extend(["--profile", self.profile])
        env = os.environ.copy()
        env.update(self._get_env())
        subprocess.run(cmd, capture_output=True, env=env)

    def get_staging_dir(self, name: str) -> str:
        """Get the staging directory for a run."""
        return str(Path(self.config_dir) / "runs" / name / "work" / "staging")

    def verify_scenario_output(
        self,
        scenario: str,
        run_name: str,
    ) -> None:
        """Verify that scenario output is correct.

        Checks the staging directory where worker changes are made.
        """
        target_dir = self.get_staging_dir(run_name)

        if scenario == "hello_world":
            self._verify_hello_world(target_dir)
        elif scenario == "calculator":
            self._verify_calculator(target_dir)
        elif scenario == "noop":
            # Noop scenario doesn't produce output to verify
            pass
        else:
            # For other scenarios, just check the run completed
            pass

    def _verify_hello_world(self, work_dir: str) -> None:
        """Verify hello_world scenario output."""
        # Check locally
        hello_py = Path(work_dir) / "hello.py"
        if not hello_py.exists():
            pytest.fail(f"hello.py not found in {work_dir}")

        result = subprocess.run(
            ["python3", "hello.py"],
            cwd=work_dir,
            capture_output=True,
            text=True,
        )
        if result.stdout.strip() != "Hello, World!":
            pytest.fail(
                f"Unexpected output: '{result.stdout.strip()}', "
                "expected 'Hello, World!'"
            )

    def _verify_calculator(self, work_dir: str) -> None:
        """Verify calculator scenario output."""
        result = subprocess.run(
            ["python3", "-m", "pytest", "-v"],
            cwd=work_dir,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            pytest.fail(f"Calculator tests failed:\n{result.stdout}\n{result.stderr}")
