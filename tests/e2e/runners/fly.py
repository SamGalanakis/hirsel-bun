"""Fly.io runner - executes on Fly.io machines."""

import os
import shutil
from typing import TYPE_CHECKING

import pytest

from .base import BaseRunner, RunnerConfig

if TYPE_CHECKING:
    from orchestrator import Orchestrator


class FlyRunner(BaseRunner):
    """Fly.io runner - executes on Fly.io machines."""

    config = RunnerConfig(
        name="fly",
        supports_pause_resume=True,
        supports_container=True,
        timeout=600,  # Fly machine startup takes time
        markers=["slow", "fly"],
    )

    def skip_if_unavailable(self) -> None:
        if not shutil.which("fly"):
            pytest.skip("fly CLI not found")

        if not os.environ.get("FLY_API_TOKEN"):
            pytest.skip("FLY_API_TOKEN not set")

    def configure(self, orchestrator: "Orchestrator") -> None:
        fly_app = os.environ.get("FLY_WORKERS_APP", "hirsel-workers")
        fly_token = os.environ.get("FLY_API_TOKEN")

        if not fly_token:
            pytest.skip("FLY_API_TOKEN not set")

        config = f"""
[runners.fly]
[runners.fly.host]
type = "fly"
app = "{fly_app}"
api_token = "{fly_token}"
cpus = 1
memory_mb = 1024

[runners.fly.container]
image = "debian:bookworm-slim"
"""
        orchestrator.write_config(config)

    def verify_output(
        self, orchestrator: "Orchestrator", run_name: str, scenario: str
    ) -> None:
        # Output is in the run's staging directory
        orchestrator.verify_scenario_output(scenario, run_name)
