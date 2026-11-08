"""Unit tests for the sidecar's pure logic (no camoufox, no network)."""

import json

from camfox_sidecar.mapping import decode_chunks, proxy_kwarg, summary  # noqa: F401
from camfox_sidecar.protocol import fail, finalize, ok  # noqa: F401


class TestDecodeChunks:
    def test_single_chunk(self):
        assert decode_chunks({"CAMOU_CONFIG_1": '{"a":1}'}, "CAMOU_CONFIG_") == '{"a":1}'

    def test_multiple_chunks_join_in_order(self):
        env = {"CAMOU_CONFIG_2": "world", "CAMOU_CONFIG_1": "hello "}
        assert decode_chunks(env, "CAMOU_CONFIG_") == "hello world"

    def test_missing(self):
        assert decode_chunks({}, "CAMOU_CONFIG_") is None

    def test_stops_at_first_gap(self):
        env = {"CAMOU_CONFIG_1": "a", "CAMOU_CONFIG_3": "c"}
        assert decode_chunks(env, "CAMOU_CONFIG_") == "a"


class TestProxyKwarg:
    def test_http(self):
        out = proxy_kwarg({"kind": "http", "host": "1.2.3.4", "port": 8080})
        assert out == {"server": "http://1.2.3.4:8080"}

    def test_socks5_with_auth(self):
        out = proxy_kwarg(
            {"kind": "socks5", "host": "p.example.com", "port": 1080, "username": "u", "password": "p@ss"}
        )
        assert out == {"server": "socks5://p.example.com:1080", "username": "u", "password": "p@ss"}

    def test_none_kind(self):
        assert proxy_kwarg({"kind": "none", "host": "h", "port": 1}) is None

    def test_missing_port(self):
        assert proxy_kwarg({"kind": "http", "host": "h"}) is None

    def test_empty(self):
        assert proxy_kwarg({}) is None


class TestSummary:
    def test_fields_extracted(self):
        config = {
            "navigator.userAgent": "Mozilla/5.0",
            "navigator.platform": "Win32",
            "screen.width": 1920,
            "screen.height": 1080,
            "window.outerWidth": 1382,
            "window.outerHeight": 744,
            "timezone": "Asia/Shanghai",
            "locale:all": "zh-CN",
            "webrtc:ipv4": "1.2.3.4",
        }
        s = summary(config)
        assert s["ua"] == "Mozilla/5.0"
        assert s["platform"] == "Win32"
        assert s["screen"] == [1920, 1080]
        assert s["window"] == [1382, 744]
        assert s["timezone"] == "Asia/Shanghai"
        assert s["locale"] == "zh-CN"
        # The exit IP is only reported for geoip-resolved identities, which is
        # exactly when the timezone key is present.
        assert s["ip"] == "1.2.3.4"

    def test_no_timezone_no_ip(self):
        s = summary({"webrtc:ipv4": "1.2.3.4"})
        assert s["ip"] is None


class TestResponses:
    def test_ok_shape(self):
        assert ok(x=1) == {"ok": True, "x": 1}

    def test_fail_shape(self):
        out = fail("boom")
        assert out["ok"] is False
        assert out["error"] == "boom"

    def test_finalize_wraps_bare_success(self):
        assert finalize({"value": 1}) == {"ok": True, "value": 1}

    def test_finalize_keeps_explicit_ok(self):
        assert finalize(fail("boom"))["ok"] is False

    def test_response_is_one_json_line(self):
        line = json.dumps(fail("中文 error", detail="x"), ensure_ascii=False)
        assert "\n" not in line
        assert json.loads(line)["error"] == "中文 error"
