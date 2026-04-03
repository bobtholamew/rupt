import pytest

@pytest.mark.skip
def test_skipped():
    pass

@pytest.mark.slow
def test_slow():
    pass

@pytest.mark.parametrize("x,y", [(1, 2), (3, 4), (5, 6)])
def test_parametrized(x, y):
    assert x < y

class TestWithMarkers:
    @pytest.mark.xfail
    def test_expected_failure(self):
        assert False

    @pytest.mark.parametrize("val", [1, 2, 3])
    def test_params(self, val):
        assert val > 0
