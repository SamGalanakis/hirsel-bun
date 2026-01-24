"""SSH runner - executes via SSH connection."""

from typing import TYPE_CHECKING

from .base import BaseRunner, RunnerConfig

if TYPE_CHECKING:
    from orchestrator import Orchestrator


class SshRunner(BaseRunner):
    """SSH runner - executes via SSH connection to remote host."""

    config = RunnerConfig(
        name="ssh",
        supports_pause_resume=True,
        supports_container=False,
        timeout=300,
    )

    def __init__(self, orchestrator: "Orchestrator"):
        super().__init__(orchestrator)
        # SSH runner uses loopback (SSH to localhost) for testing
        self.remote_work_dir = "/tmp/hirsel-remote"

    def configure(self, orchestrator: "Orchestrator") -> None:
        # SSH runner is configured via --remote flag, not config file
        # For testing, we set up SSH keys for localhost loopback
        orchestrator.setup_ssh_loopback()

    def get_cli_args(self) -> list[str]:
        """Get CLI arguments for SSH runner."""
        return ["--remote", "root@localhost:1"]

    def verify_output(
        self, orchestrator: "Orchestrator", run_name: str, scenario: str
    ) -> None:
        # Output is in the run's staging directory
        orchestrator.verify_scenario_output(scenario, run_name)
