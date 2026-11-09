"""Tests for identity summary and WebRTC option mapping."""

import json

import pytest

from camfox_sidecar.mapping import canvas_covered, identity_id, summary
from camfox_sidecar.prepare import _apply_webrtc


CONFIG = {
    "navigator.userAgent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:158.0) Firefox/158.0",
    "navigator.platform": "Win32",
    "navigator.oscpu": "Windows NT 10.0; Win64; x64",
    "navigator.hardwareConcurrency": 8,
    "screen.width": 1920,
    "screen.height": 1080,
    "screen.availWidth": 1920,
    "screen.availHeight": 1032,
    "window.outerWidth": 1920,
    "window.outerHeight": 1032,
    "window.devicePixelRatio": 1.25,
    "timezone": "Asia/Shanghai",
    "locale:all": "zh-CN",
    "fonts": ["Arial", "Consolas"],
    "voices": [{"name": "v"}],
    "mediaDevices:micros": 1,
    "mediaDevices:webcams": 1,
    "mediaDevices:speakers": 2,
    "webGl:vendor": "Google Inc. (Intel)",
    "webGl:renderer": "ANGLE (Intel, Intel(R) HD Graphics Direct3D11 vs_5_0 ps_5_0)",
}


class TestIdentityId:
    def test_stable_for_same_config(self):
        assert identity_id(CONFIG) == identity_id(dict(reversed(list(CONFIG.items()))))

    def test_changes_when_identity_changes(self):
        changed = dict(CONFIG, **{"navigator.hardwareConcurrency": 16})
        assert identity_id(CONFIG) != identity_id(changed)

    def test_is_16_hex(self):
        value = identity_id(CONFIG)
        assert len(value) == 16
        int(value, 16)


class TestSummary:
    def test_includes_new_fields(self):
        s = summary(CONFIG)
        assert s["identityId"] == identity_id(CONFIG)
        assert s["oscpu"].startswith("Windows NT")
        assert s["availScreen"] == [1920, 1032]
        assert s["colorDepth"] is None
        assert s["gpuRenderer"].startswith("ANGLE (Intel")
        assert s["fontsCount"] == 2
        assert s["voicesCount"] == 1
        assert s["mediaDevices"] == [1, 1, 2]

    def test_camel_case_keys(self):
        s = summary(CONFIG)
        assert "identityId" in s and "identity_id" not in s
        assert "gpuVendor" in s and "gpu_vendor" not in s

    def test_missing_pair_is_none_not_null_padded(self):
        # innerWindow used to be [null, null]: the Rust side decodes these as
        # Vec<u32>, failed, and dropped the whole summary, so every profile
        # showed "no fingerprint" although its config had been stored.
        s = summary(CONFIG)
        assert s["innerWindow"] is None
        assert s["window"] == [1920, 1032]
        for key in ("screen", "availScreen", "window", "innerWindow"):
            value = s[key]
            assert value is None or all(v is not None for v in value), (key, value)
        json.dumps(s)


class TestCanvasCovered:
    def test_absent_override_is_uncovered(self):
        assert canvas_covered({}) is False
        assert canvas_covered({"privacy.baselineFingerprintingProtection.overrides": None}) is False

    def test_canvas_target_means_covered(self):
        prefs = {
            "privacy.baselineFingerprintingProtection.overrides":
                "+CanvasImageExtractionPrompt,+CanvasExtractionBeforeUserInputIsBlocked"
        }
        assert canvas_covered(prefs) is True

    def test_other_overrides_do_not_cover(self):
        prefs = {"privacy.baselineFingerprintingProtection.overrides": "+CanvasRandomization"}
        assert canvas_covered(prefs) is False


class TestWebrtcMapping:
    def test_block(self):
        kwargs = {"config": {}}
        _apply_webrtc({"mode": "block"}, kwargs)
        assert kwargs["block_webrtc"] is True

    def test_custom_sets_ipv4_and_disables_ipv6(self):
        kwargs = {"config": {}}
        _apply_webrtc({"mode": "custom", "ipv4": "203.0.113.10"}, kwargs)
        assert kwargs["config"]["webrtc:ipv4"] == "203.0.113.10"
        assert kwargs["config"]["network:ipv4Only"] is True

    def test_custom_requires_an_address(self):
        with pytest.raises(ValueError):
            _apply_webrtc({"mode": "custom"}, {"config": {}})

    def test_custom_rejects_a_malformed_address(self):
        with pytest.raises(ValueError):
            _apply_webrtc({"mode": "custom", "ipv4": "999.1.1.1"}, {"config": {}})

    def test_auto_leaves_webrtc_alone(self):
        kwargs = {"config": {}}
        _apply_webrtc({"mode": "auto"}, kwargs)
        assert kwargs == {"config": {}}

    def test_unknown_mode_fails_loudly(self):
        with pytest.raises(ValueError):
            _apply_webrtc({"mode": "sometimes"}, {"config": {}})
