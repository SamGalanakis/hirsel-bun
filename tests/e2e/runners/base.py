"""Base class for runner implementations."""

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from orchestrator import Orchestrator


@dataclass
class RunnerConfig:
    """Base configuration for a runner."""

    name: str
    supports_pause_resume: bool = True
    supports_container: bool = True
    timeout: int = 300
    markers: list[str] = field(default_factory=list)


class BaseRunner(ABC):
    """Base class for runner implementations."""

    config: RunnerConfig

    def __init__(self, orchestrator: "Orchestrator"):
        self.orchestrator = orchestrator

    @property
    def name(self) -> str:
        return self.config.name

    @property
    def supports_pause_resume(self) -> bool:
        return self.config.supports_pause_resume

    @property
    def timeout(self) -> int:
        return self.config.timeout

    @property
    def markers(self) -> list[str]:
        return self.config.markers

    @abstractmethod
    def configure(self, orchestrator: "Orchestrator") -> None:
        """Write runner config to orchestrator's config.toml."""

    @abstractmethod
    def verify_output(
        self, orchestrator: "Orchestrator", run_name: str, scenario: str
    ) -> None:
        """Verify the scenario output is correct."""

    def get_verification_host(
        self, orchestrator: "Orchestrator", run_name: str
    ) -> str:
        """Get the host where output should be verified (orchestrator or worker)."""
        return orchestrator.host  # Default: check on orchestrator

    def skip_if_unavailable(self) -> None:
        """Skip test if runner prerequisites are not met. Override in subclasses."""
