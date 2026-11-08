"""Tests for the host-GPU disk cache wrapper (no kernel launched)."""

import json
import time

import camoufox.host_rendering as hr

from camfox_sidecar import host_gpu_cache

ENV = ()
PREFS = (("pref", 1),)
EXE = "C:/k/camoufox.exe"
GPU = ("Intel", "Intel(R) HD Graphics")


class FakeUpstream:
    """Stand-in for the upstream lru_cache wrapper."""

    def __init__(self):
        self.calls = 0

    @property
    def __wrapped__(self):
        return self._probe

    def _probe(self, exe, headless, env, prefs):
        self.calls += 1
        return GPU

    def cache_clear(self):
        pass


def install_fake(monkeypatch, tmp_path):
    fake = FakeUpstream()
    monkeypatch.setattr(host_gpu_cache, "cache_path", lambda: tmp_path / "gpu-cache.json")
    monkeypatch.setattr(hr, "_host_gpu", fake)
    host_gpu_cache._WRAPPED = None  # reinstall against the fake
    host_gpu_cache.install()
    return fake


def probe():
    return hr._host_gpu(EXE, False, ENV, PREFS)


def test_first_call_probes_then_cache_hit(monkeypatch, tmp_path):
    fake = install_fake(monkeypatch, tmp_path)
    assert probe() == GPU
    assert fake.calls == 1
    assert probe() == GPU
    assert fake.calls == 1  # second read came from disk
    data = json.loads((tmp_path / "gpu-cache.json").read_text(encoding="utf-8"))
    assert list(data.values())[0]["gpu"] == list(GPU)


def test_expired_entry_probes_again(monkeypatch, tmp_path):
    fake = install_fake(monkeypatch, tmp_path)
    cache_file = tmp_path / "gpu-cache.json"
    key = host_gpu_cache._key(EXE, False, PREFS)
    stale = {key: {"t": time.time() - host_gpu_cache.CACHE_TTL_SECONDS - 60, "gpu": list(GPU)}}
    cache_file.write_text(json.dumps(stale), encoding="utf-8")
    assert probe() == GPU
    assert fake.calls == 1  # stale entry ignored, real probe ran


def test_none_result_is_cached(monkeypatch, tmp_path):
    class NoneProbe(FakeUpstream):
        def _probe(self, exe, headless, env, prefs):
            self.calls += 1
            return None

    fake = NoneProbe()
    monkeypatch.setattr(host_gpu_cache, "cache_path", lambda: tmp_path / "gpu-cache.json")
    monkeypatch.setattr(hr, "_host_gpu", fake)
    host_gpu_cache._WRAPPED = None
    host_gpu_cache.install()
    assert probe() is None
    assert probe() is None
    assert fake.calls == 1  # the None (WebGL-less host) is a valid cached value


def test_install_is_idempotent(monkeypatch, tmp_path):
    install_fake(monkeypatch, tmp_path)
    installed = hr._host_gpu
    host_gpu_cache.install()
    assert hr._host_gpu is installed  # second install did not double-wrap
