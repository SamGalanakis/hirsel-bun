"""Pytest configuration and fixtures for E2E tests."""

import os
from pathlib import Path

import pytest
from dotenv import load_dotenv

from orchestrator import Orchestrator
from runners import (
    DockerRunner,
    FlyRunner,
    LocalRunner,
    SshRunner,
)

# Load .env from tests/e2e/ or project root
load_dotenv(Path(__file__).parent / ".env")
load_dotenv(Path(__file__).parent.parent.parent / ".env")

RUNNERS = {
    "local": LocalRunner,
    "docker": DockerRunner,
    "ssh": SshRunner,
    "fly": FlyRunner,
}

SCENARIOS = ["hello_world", "calculator", "noop"]

# Default to fast scenarios that don't require Claude API
DEFAULT_RUNNERS = ["local"]
DEFAULT_SCENARIOS = ["noop"]


def pytest_addoption(parser: pytest.Parser) -> None:
    """Add custom command-line options."""
    parser.addoption(
        "--runner",
        action="append",
        default=[],
        help="Runners to test (can be specified multiple times)",
    )
    parser.addoption(
        "--scenario",
        action="append",
        default=[],
        help="Scenarios to test (can be specified multiple times)",
    )
    parser.addoption(
        "--profile",
        default="local",
        help="Orchestrator profile name (from config.toml profiles)",
    )
    parser.addoption(
        "--workers",
        type=int,
        default=1,
        help="Number of hirsel workers per run (worker_scale)",
    )
    parser.addoption(
        "--hirsel-dir",
        default=None,
        help="Override hirsel config directory (default: temp dir for isolation)",
    )


def pytest_generate_tests(metafunc: pytest.Metafunc) -> None:
    """Generate test matrix from CLI options or defaults."""
    if "runner_name" in metafunc.fixturenames:
        runners = metafunc.config.getoption("runner") or DEFAULT_RUNNERS
        # Validate runner names
        for r in runners:
            if r not in RUNNERS:
                pytest.fail(f"Unknown runner: {r}. Valid: {list(RUNNERS.keys())}")
        metafunc.parametrize("runner_name", runners)

    if "scenario" in metafunc.fixturenames:
        scenarios = metafunc.config.getoption("scenario") or DEFAULT_SCENARIOS
        # Validate scenario names
        scenarios_dir = Path(__file__).parent.parent / "scenarios"
        for s in scenarios:
            if not (scenarios_dir / s).is_dir():
                pytest.fail(f"Unknown scenario: {s}")
        metafunc.parametrize("scenario", scenarios)


@pytest.fixture(scope="session")
def hirsel_binary() -> str:
    """Path to hirsel binary."""
    path = os.environ.get(
        "HIRSEL_BINARY",
        str(Path(__file__).parent.parent.parent / "src-tauri/target/release/hirsel"),
    )

    # Also check debug build
    if not Path(path).exists():
        debug_path = path.replace("/release/", "/debug/")
        if Path(debug_path).exists():
            path = debug_path

    if not Path(path).exists():
        pytest.skip(f"hirsel binary not found: {path}")

    return path


@pytest.fixture(scope="session")
def orchestrator(request: pytest.FixtureRequest, hirsel_binary: str) -> Orchestrator:
    """Set up orchestrator based on profile."""
    profile = request.config.getoption("profile")
    worker_scale = request.config.getoption("workers")
    hirsel_dir = request.config.getoption("hirsel_dir")
    orch = Orchestrator(
        profile=profile,
        binary=hirsel_binary,
        worker_scale=worker_scale,
        hirsel_dir=hirsel_dir,
    )
    orch.setup()
    yield orch
    orch.teardown()


@pytest.fixture
def runner(runner_name: str, orchestrator: Orchestrator):
    """Get configured runner instance."""
    runner_cls = RUNNERS[runner_name]
    runner_instance = runner_cls(orchestrator)

    # Skip if runner prerequisites not met
    runner_instance.skip_if_unavailable()

    return runner_instance


@pytest.fixture
def run_name(runner_name: str, scenario: str, orchestrator: Orchestrator) -> str:
    """Generate a unique run name for this test."""
    return f"e2e-{runner_name}-{scenario}-{orchestrator.run_id}"
