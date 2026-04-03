import pytest

@pytest.fixture
def sample_data():
    return [1, 2, 3]

@pytest.fixture(scope="session")
def db_connection():
    return "fake_connection"
