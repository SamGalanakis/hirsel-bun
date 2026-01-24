"""Main parametrized E2E tests for hirsel runs."""

import pytest

from orchestrator import Orchestrator
from runners.base import BaseRunner
from runners.ssh import SshRunner


class TestRunScenario:
    """Main parametrized test for running scenarios with different runners."""

    def test_run_completes(
        self,
        runner: BaseRunner,
        scenario: str,
        orchestrator: Orchestrator,
        run_name: str,
    ) -> None:
        """Run a scenario and verify it completes successfully."""
        # Configure runner on orchestrator
        runner.configure(orchestrator)

        # Copy spec to work directory
        spec_path, eval_path = orchestrator.copy_spec(scenario)

        # Build extra args for specific runners
        extra_args = []
        if isinstance(runner, SshRunner):
            extra_args = runner.get_cli_args()

        # Start run
        orchestrator.start_run(
            name=run_name,
            spec=spec_path,
            eval_spec=eval_path,
            runner=runner.name if not isinstance(runner, SshRunner) else "local",
            extra_args=extra_args,
        )

        # Wait for completion with runner-specific timeout
        status = orchestrator.wait_for_run(run_name, timeout=runner.timeout)

        # Accept completed, delivered, pass, fail, or done as success
        # (fail means eval failed, but run completed)
        assert status in ("completed", "delivered", "pass", "fail", "done"), (
            f"Unexpected final status: {status}"
        )

        # Verify output
        runner.verify_output(orchestrator, run_name, scenario)


DONE_STATUSES = ("completed", "delivered", "pass", "fail", "done")


class TestPauseResume:
    """Tests for pause and resume functionality."""

    @pytest.mark.slow
    def test_pause_resume(
        self,
        runner: BaseRunner,
        orchestrator: Orchestrator,
    ) -> None:
        """Test pause and resume functionality."""
        if not runner.supports_pause_resume:
            pytest.skip(f"{runner.name} doesn't support pause/resume")

        run_name = f"e2e-pause-{runner.name}-{orchestrator.run_id}"

        # Configure runner
        runner.configure(orchestrator)

        # Copy a simple spec
        spec_path, _ = orchestrator.copy_spec("hello_world")

        # Build extra args for specific runners
        extra_args = []
        if isinstance(runner, SshRunner):
            extra_args = runner.get_cli_args()

        # Start run
        orchestrator.start_run(
            name=run_name,
            spec=spec_path,
            runner=runner.name if not isinstance(runner, SshRunner) else "local",
            extra_args=extra_args,
        )

        # Wait for running state, checking for early completion
        if not orchestrator.wait_for_working_or_done(run_name, timeout=60):
            pytest.skip("Run completed before pause test could run")

        # Pause
        orchestrator.pause_run(run_name)

        # Wait for paused status, checking for completion
        if not orchestrator.wait_for_paused_or_done(run_name, timeout=30):
            pytest.skip("Run completed during pause")

        assert orchestrator.get_status(run_name) == "paused"

        # Resume
        orchestrator.resume_run(run_name)

        # Wait for completion
        status = orchestrator.wait_for_run(run_name, timeout=runner.timeout)
        assert status in DONE_STATUSES


class TestDockerLifecycle:
    """Docker-specific lifecycle tests."""

    @pytest.mark.docker
    @pytest.mark.timeout(600)  # Docker container startup takes significant time
    def test_container_cleanup(
        self,
        orchestrator: Orchestrator,
    ) -> None:
        """Test that docker containers are properly cleaned up after runs."""
        import subprocess

        from runners import DockerRunner

        runner = DockerRunner(orchestrator)
        runner.skip_if_unavailable()
        runner.configure(orchestrator)

        run_name = f"e2e-docker-cleanup-{orchestrator.run_id}"
        spec_path, _ = orchestrator.copy_spec("noop")

        # Start run
        orchestrator.start_run(
            name=run_name,
            spec=spec_path,
            runner="docker",
        )

        # Wait for completion with longer timeout for docker
        orchestrator.wait_for_run(run_name, timeout=500)

        # Check that no containers are left running
        result = subprocess.run(
            ["docker", "ps", "-q", "--filter", f"name=hirsel-{run_name}"],
            capture_output=True,
            text=True,
        )
        remaining = result.stdout.strip()
        assert not remaining, f"Docker containers still running: {remaining}"
