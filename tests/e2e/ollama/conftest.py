from __future__ import annotations

from pathlib import Path

import pytest

from tests.common.helpers import find_free_port, render_standalone_yaml, start_nyro_server, stop_nyro_server, wait_until_ready

CONFIG_TEMPLATE = Path(__file__).parent / "standalone.yaml"


@pytest.fixture(scope="module")
def ollama_base_url(nyro_binary: Path) -> str:
    proxy_port = find_free_port()
    config_path = render_standalone_yaml(CONFIG_TEMPLATE, proxy_port)
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
