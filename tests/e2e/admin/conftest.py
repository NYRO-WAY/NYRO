from __future__ import annotations

import subprocess
import tempfile
import threading
from pathlib import Path

import pytest

from tests.common.helpers import find_free_port, minimal_mock_provider, stop_nyro_server, wait_until_ready


def start_nyro_full(
    *,
    proxy_port: int,
    admin_port: int,
    admin_token: str,
    data_dir: str,
    nyro_binary: Path,
) -> tuple[subprocess.Popen[str], list[str]]:
    logs: list[str] = []
    proc = subprocess.Popen(
        [
            str(nyro_binary),
            "--proxy-host",
            "127.0.0.1",
            "--proxy-port",
            str(proxy_port),
            "--admin-host",
            "127.0.0.1",
            "--admin-port",
            str(admin_port),
            "--data-dir",
            data_dir,
            "--admin-token",
            admin_token,
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    def _drain() -> None:
        assert proc.stdout is not None
        for line in proc.stdout:
            logs.append(line.rstrip("\n"))

    threading.Thread(target=_drain, name="nyro-admin-log", daemon=True).start()
    return proc, logs


@pytest.fixture(scope="module")
def admin_env(nyro_binary: Path) -> dict[str, str]:
    admin_token = "admin-e2e-token"
    mock_port = find_free_port()
    proxy_port = find_free_port()
    admin_port = find_free_port()

    mock_server, _ = minimal_mock_provider(mock_port)
    proc = None
    logs: list[str] | None = None

    try:
        with tempfile.TemporaryDirectory(prefix="nyro-admin-e2e-") as data_dir:
            proc, logs = start_nyro_full(
                proxy_port=proxy_port,
                admin_port=admin_port,
                admin_token=admin_token,
                data_dir=data_dir,
                nyro_binary=nyro_binary,
            )
            admin_base = f"http://127.0.0.1:{admin_port}"
            proxy_base = f"http://127.0.0.1:{proxy_port}"
            auth_headers = {"authorization": f"Bearer {admin_token}"}

            wait_until_ready(
                f"{admin_base}/api/v1/status",
                timeout=40.0,
                headers=auth_headers,
            )

            yield {
                "admin": admin_base,
                "proxy": proxy_base,
                "mock": f"http://127.0.0.1:{mock_port}",
                "auth": auth_headers,
            }
    finally:
        if proc is not None and logs is not None:
            stop_nyro_server(proc, logs)
        mock_server.shutdown()
        mock_server.server_close()
