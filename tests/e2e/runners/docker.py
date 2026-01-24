"""Docker runner - executes in a local Docker container."""

import shutil
import subprocess
from typing import TYPE_CHECKING

import pytest

from .base import BaseRunner, RunnerConfig

if TYPE_CHECKING:
    from orchestrator import Orchestrator


class DockerRunner(BaseRunner):
    """Docker runner - executes in a local Docker container."""

    config = RunnerConfig(
        name="docker",
        supports_pause_resume=True,
        supports_container=True,
        timeout=500,  # Docker container startup takes significant time
        markers=["docker"],
    )

    def skip_if_unavailable(self) -> None:
        if not shutil.which("docker"):
            pytest.skip("docker CLI not found")

        try:
            result = subprocess.run(
                ["docker", "info"],
                capture_output=True,
                timeout=10,
            )
            if result.returncode != 0:
                pytest.skip("docker daemon not running")
        except subprocess.TimeoutExpired:
            pytest.skip("docker daemon not responding")

    def configure(self, orchestrator: "Orchestrator") -> None:
        config = """
[runners.docker]
[runners.docker.host]
type = "local"

[runners.docker.container]
image = "buildpack-deps:noble"
"""
        orchestrator.write_config(config)

    def verify_output(
        self, orchestrator: "Orchestrator", run_name: str, scenario: str
    ) -> None:
        # Output is in the run's staging directory
        orchestrator.verify_scenario_output(scenario, run_name)

    def cleanup_containers(self, run_name: str) -> None:
        """Stop and remove any containers from this run."""
        subprocess.run(
            ["docker", "ps", "-q", "--filter", f"name=hirsel-{run_name}"],
            capture_output=True,
        )
        # Stop containers
        result = subprocess.run(
            ["docker", "ps", "-q", "--filter", f"name=hirsel-{run_name}"],
            capture_output=True,
            text=True,
        )
        if result.stdout.strip():
            container_ids = result.stdout.strip().split("\n")
            for cid in container_ids:
                subprocess.run(
                    ["docker", "stop", "-t", "10", cid],
                    capture_output=True,
                )
                subprocess.run(
                    ["docker", "rm", "-f", cid],
                    capture_output=True,
                )
