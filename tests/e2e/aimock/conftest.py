from __future__ import annotations

import os
import subprocess
from pathlib import Path

import pytest

from tests.common.helpers import (
    find_free_port,
    is_port_free,
    render_standalone_yaml,
    start_nyro_server,
    stop_nyro_server,
    wait_until_ready,
)

AIMOCK_IMAGE = os.environ.get("AIMOCK_IMAGE", "ghcr.io/copilotkit/aimock:latest")
FIXTURES_DIR = Path(__file__).parent / "fixtures"
CONFIG_TEMPLATE = Path(__file__).parent / "standalone.yaml"
SCHEMA_DIRS: list[tuple[str, str]] = [
    ("AIMOCK_OPENAI_COMPLETIONS_PORT", "openai-completions"),
    ("AIMOCK_OPENAI_RESPONSES_PORT", "openai-responses"),
    ("AIMOCK_ANTHROPIC_PORT", "anthropic-messages"),
    ("AIMOCK_GEMINI_PORT", "google-generatecontent"),
]
PREFERRED_PORTS = [4010, 4011, 4012, 4013]


def choose_ports() -> list[int]:
    if all(is_port_free(port) for port in PREFERRED_PORTS):
        return list(PREFERRED_PORTS)
    return [find_free_port() for _ in PREFERRED_PORTS]


def start_aimock(port: int, fixture_subdir: str, *, container_name: str) -> None:
    host_dir = str((FIXTURES_DIR / fixture_subdir).resolve())
    cmd = [
        "docker",
        "run",
        "--rm",
        "-d",
        "--name",
        container_name,
        "-p",
        f"127.0.0.1:{port}:4010",
        "-v",
        f"{host_dir}:/fixtures:ro",
        AIMOCK_IMAGE,
        "-f",
        "/fixtures",
        "--host",
        "0.0.0.0",
    ]
    subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)


def stop_containers(names: list[str]) -> None:
    for name in names:
        subprocess.run(
            ["docker", "rm", "-f", name],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )


@pytest.fixture(scope="module")
def aimock_cluster() -> dict[str, int]:
    aimock_ports = choose_ports()
    port_map = {key: port for (key, _), port in zip(SCHEMA_DIRS, aimock_ports)}
    container_names = [f"nyro-aimock-{idx}" for idx in range(len(SCHEMA_DIRS))]
    stop_containers(container_names)

    try:
        for (_, subdir), port, name in zip(SCHEMA_DIRS, aimock_ports, container_names):
            start_aimock(port, subdir, container_name=name)
        for port in aimock_ports:
            wait_until_ready(f"http://127.0.0.1:{port}/health", timeout=30.0)
        yield port_map
    finally:
        stop_containers(container_names)


@pytest.fixture(scope="module")
def aimock_base_url(aimock_cluster: dict[str, int], nyro_binary: Path) -> str:
    proxy_port = find_free_port()
    config_path = render_standalone_yaml(CONFIG_TEMPLATE, proxy_port, aimock_cluster)
    proc, logs = start_nyro_server(config_path, nyro_binary=nyro_binary)
    base = f"http://127.0.0.1:{proxy_port}"

    try:
        wait_until_ready(f"{base}/v1/chat/completions", timeout=30.0)
        yield base
    finally:
        stop_nyro_server(proc, logs)
        try:
            config_path.unlink(missing_ok=True)
        except OSError:
            pass
