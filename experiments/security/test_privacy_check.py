from privacy_check import canary_hits, code_lines, connects, network_crates, opened_outside

TREE = """kestrel-cli v0.1.0
├── kestrel-core v0.1.0
│   └── serde v1.0.228
├── llama-cpp-2 v0.1.157
└── reqwest v0.12.0
    └── hyper v1.5.0
"""

TRACE = """1234  socket(AF_UNIX, SOCK_STREAM|SOCK_CLOEXEC, 0) = 3
1234  connect(3, {sa_family=AF_UNIX, sun_path="/run/user/1000/bus"}, 23) = 0
1240  connect(5, {sa_family=AF_INET, sin_port=htons(443), sin_addr=inet_addr("203.0.113.7")}, 16) = -1 ENETUNREACH
1241  connect(6, {sa_family=AF_INET6, sin6_port=htons(80)}, 28) = -1 ENETUNREACH
"""


def test_network_crates_are_found_in_a_dependency_tree():
    assert network_crates(TREE) == ["reqwest v0.12.0", "hyper v1.5.0"]
    assert network_crates("kestrel-cli v0.1.0\n└── serde v1.0.228\n") == []


def test_only_inet_connects_count_as_network():
    c = connects(TRACE)
    assert c["inet"] == 2 and c["unix"] == 1


def test_code_lines_skip_comments():
    src = '//! see https://example.org\nlet a = 1; // http://x\nlet url = "https://api.example.com";\n'
    assert code_lines(src, r"https?://") == [(3, 'let url = "https://api.example.com";')]


def test_opened_paths_outside_the_allowed_roots():
    trace = ('10 openat(AT_FDCWD, "/home/u/Kestrel/models/m.gguf", O_RDONLY) = 3\n'
             '10 openat(AT_FDCWD, "/etc/passwd", O_RDONLY) = 4\n'
             '10 openat(AT_FDCWD, "/usr/lib64/libc.so.6", O_RDONLY|O_CLOEXEC) = 5\n'
             '10 openat(AT_FDCWD, "/home/u/secret.txt", O_RDONLY) = -1 ENOENT\n')
    assert opened_outside(trace, ["/home/u/Kestrel", "/usr/lib64"]) == ["/etc/passwd"]  # failed opens don't count
    rel = '10 openat(AT_FDCWD, "models/m.gguf", O_RDONLY) = 3\n10 openat(AT_FDCWD, "../notes.txt", O_RDONLY) = 4\n'
    assert opened_outside(rel, ["/home/u/Kestrel"], cwd="/home/u/Kestrel") == ["/home/u/notes.txt"]


def test_canary_hits_name_the_files_that_contain_it(tmp_path):
    (tmp_path / "telemetry.jsonl").write_text('{"tier": 3, "ms": 5}\n')
    (tmp_path / "rows.jsonl").write_text('{"id": "x", "note": "ZQCANARY7F3A"}\n')
    assert canary_hits([tmp_path / "telemetry.jsonl", tmp_path / "rows.jsonl"], "zqcanary7f3a") == [
        str(tmp_path / "rows.jsonl")]
