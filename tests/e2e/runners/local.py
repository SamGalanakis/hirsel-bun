"""Local runner - executes directly on orchestrator."""

from typing import TYPE_CHECKING

from .base import BaseRunner, RunnerConfig

if TYPE_CHECKING:
    from orchestrator import Orchestrator


class LocalRunner(BaseRunner):
    """Local runner - executes directly on orchestrator machine."""

    config = RunnerConfig(
        name="local",
        supports_pause_resume=True,
        supports_container=False,
        timeout=300,
    )

    def configure(self, orchestrator: "Orchestrator") -> None:
        # Local runner needs no special config - it's the default
        pass

    def verify_output(
        self, orchestrator: "Orchestrator", run_name: str, scenario: str
    ) -> None:
        # Output is in the run's staging directory
        orchestrator.verify_scenario_output(scenario, run_name)
