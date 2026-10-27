from sandbox import run_python


def test_code_runs_with_its_input_and_output():
    r = run_python("import sys\nprint(sum(int(x) for x in sys.stdin.read().split()))", stdin="1 2 3\n")
    assert (r.ok, r.stdout, r.timed_out) == (True, "6\n", False)


def test_an_error_comes_back_as_its_traceback():
    r = run_python("def f():\n    return 1 / 0\nf()")
    assert not r.ok and "ZeroDivisionError" in r.stderr and r.returncode != 0


def test_no_network_and_none_of_the_users_files():
    code = ("import os, socket\n"
            "try:\n    socket.create_connection(('1.1.1.1', 80), timeout=2); print('net')\n"
            "except OSError:\n    print('no net')\n"
            "print(os.path.exists(os.path.expanduser('~/notes')), os.path.exists('/home'))")
    r = run_python(code)
    assert r.stdout.split("\n")[:2] == ["no net", "False False"], r


def test_an_endless_loop_is_stopped_and_memory_is_capped():
    r = run_python("while True:\n    pass", timeout_s=2)
    assert r.timed_out and not r.ok
    r = run_python("x = bytearray(2_000_000_000)\nprint('allocated')")
    assert not r.ok and "MemoryError" in r.stderr


def test_files_live_only_in_the_run_s_own_folder():
    r = run_python("open('out.txt', 'w').write('hi')\nprint(open('out.txt').read())")
    assert r.stdout == "hi\n"
    r = run_python("import os\nprint(os.path.exists('out.txt'))")
    assert r.stdout == "False\n", "each run starts empty"


def test_extra_files_sit_beside_the_code():
    r = run_python("import solution\nprint(solution.double(4))", files={"solution.py": "def double(x):\n    return 2 * x\n"})
    assert r.stdout == "8\n", r


def test_only_the_standard_library_and_a_clean_stderr():
    r = run_python("import json, re, math\nprint(json.dumps([math.floor(2.5)]))")
    assert (r.stdout, r.stderr) == ("[2]\n", "")
    r = run_python("import numpy")
    assert "ModuleNotFoundError" in r.stderr
