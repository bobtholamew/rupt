def test_with_fixture(sample_data):
    assert sample_data == [1, 2, 3]

def test_with_tmp_path(tmp_path):
    f = tmp_path / "test.txt"
    f.write_text("hello")
    assert f.read_text() == "hello"

def test_stdout_capture(capsys):
    print("hello world")
    out, err = capsys.readouterr()
    assert out == "hello world\n"
