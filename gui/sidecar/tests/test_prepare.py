"""The launch bundle decides geography from the proxy exit IP."""

from camfox_sidecar.prepare import _geoip_for


class TestGeoipFor:
    def test_a_proxy_without_geoip_follows_the_exit_ip(self):
        # Otherwise the identity keeps this host's timezone and locale, which
        # contradicts the exit address and links the profile to the machine.
        assert _geoip_for({}, has_proxy=True) is True

    def test_an_explicit_timezone_wins_over_the_exit_ip(self):
        assert _geoip_for({"timezone": "Europe/Paris"}, has_proxy=True) is False

    def test_no_proxy_leaves_geoip_off(self):
        assert _geoip_for({}, has_proxy=False) is False

    def test_geoip_requested_without_a_proxy_is_kept(self):
        assert _geoip_for({"geoip": True}, has_proxy=False) is True
