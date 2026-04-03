def test_addition():
    assert 1 + 1 == 2

def test_subtraction():
    assert 2 - 1 == 1

def helper_not_a_test():
    pass

class TestMath:
    def test_multiply(self):
        assert 2 * 3 == 6

    def test_divide(self):
        assert 6 / 3 == 2

    def helper_method(self):
        pass

class NotATestClass:
    def test_should_not_collect(self):
        pass
