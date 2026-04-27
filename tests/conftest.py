from __future__ import annotations

from pathlib import Path

import pytest

from tests.common.helpers import resolve_nyro_binary


@pytest.fixture(scope="session")
def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


@pytest.fixture(scope="session")
def nyro_binary(repo_root: Path) -> Path:
    binary = resolve_nyro_binary(repo_root)
    if not binary.exists():
        pytest.skip(f"nyro-server not found at {binary}; build it first or set NYRO_BINARY")
    return binary
