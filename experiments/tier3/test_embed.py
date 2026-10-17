import numpy as np
import pytest

from embed import (
    LLAMA_EMBEDDING,
    Encoder,
    cache_key,
    embed,
    load_encoder,
    model_input,
    parse_array_output,
    run_llama_embedding,
)


def _enc(name="toy", sha="ab" * 32, prefix="query: "):
    return Encoder(name=name, path=None, sha256=sha, prefix=prefix)


def test_model_input_normalizes_like_the_runtime_then_adds_the_prefix():
    # Same text the Rust Request carries (kestrel_core::normalize), prefix first.
    assert model_input("  Remind ME to call Asha!", "query: ") == "query: remind me to call asha"
    assert model_input("Kal\tsubah  YAAD dilana.", "") == "kal subah yaad dilana"


def test_parse_array_output_ignores_log_noise_and_checks_the_count():
    out = "some log line\n[[0.1,0.2,0.3],[0.4,0.5,0.6]]\n\nllama_perf: 1 ms\n"
    x = parse_array_output(out, n_expected=2)
    assert x.dtype == np.float32
    assert x.shape == (2, 3)
    assert x[1, 2] == pytest.approx(0.6)
    with pytest.raises(ValueError, match="expected 3"):
        parse_array_output(out, n_expected=3)


def test_parse_array_output_rejects_ragged_or_missing_output():
    with pytest.raises(ValueError):
        parse_array_output("[[0.1,0.2],[0.3]]", n_expected=2)
    with pytest.raises(ValueError):
        parse_array_output("no embeddings here", n_expected=1)


def test_cache_key_changes_with_model_text_and_prefix():
    base = cache_key(_enc(), ["hello", "bye"])
    assert base == cache_key(_enc(), ["hello", "bye"])
    assert base != cache_key(_enc(sha="cd" * 32), ["hello", "bye"])
    assert base != cache_key(_enc(prefix=""), ["hello", "bye"])
    assert base != cache_key(_enc(), ["bye", "hello"])  # order is part of the key
    assert base != cache_key(_enc(), ["hello", "bye", "x"])


def test_embed_chunks_keeps_order_and_reuses_the_cache(tmp_path):
    calls = []

    def fake_runner(encoder, inputs):
        calls.append(list(inputs))
        # one row per input: [len(input), position of the first char in the alphabet]
        return np.array([[len(s), ord(s[-1]) - 96] for s in inputs], dtype=np.float32)

    texts = ["a", "bb", "ccc", "dddd", "eeeee"]
    x = embed(_enc(prefix=""), texts, chunk=2, cache_dir=tmp_path, runner=fake_runner)
    assert [len(c) for c in calls] == [2, 2, 1]
    assert x[:, 0].tolist() == [1, 2, 3, 4, 5]
    assert x[:, 1].tolist() == [1, 2, 3, 4, 5]

    again = embed(_enc(prefix=""), texts, chunk=2, cache_dir=tmp_path, runner=fake_runner)
    assert len(calls) == 3  # served from the cache, the runner was not called again
    assert np.array_equal(x, again)


def test_embed_sends_the_prefixed_normalized_text(tmp_path):
    seen = []

    def fake_runner(encoder, inputs):
        seen.extend(inputs)
        return np.zeros((len(inputs), 2), dtype=np.float32)

    embed(_enc(prefix="query: "), ["Hello Kestrel!"], cache_dir=tmp_path, runner=fake_runner)
    assert seen == ["query: hello kestrel"]


def test_load_encoder_reads_the_manifest_and_knows_each_prefix():
    e5 = load_encoder("multilingual-e5-small-f16")
    assert e5.prefix == "query: "  # e5 was trained with "query: " / "passage: " prefixes
    assert e5.sha256 == "41855d3a052d83fbcd65fa5823f8d776fe494f8f426718b424cbb13f836b5e61"
    assert e5.path.name == "multilingual-e5-small-f16.gguf"
    assert load_encoder("bge-small-en-v1.5-q8_0").prefix == ""  # no instruction for classification
    with pytest.raises(KeyError):
        load_encoder("Qwen3-1.7B-Q4_K_M")  # an SLM, not an encoder


_have_real = LLAMA_EMBEDDING.exists() and load_encoder("multilingual-e5-small-q8_0").path.exists()


@pytest.mark.skipif(not _have_real, reason="needs the local llama.cpp build and model files")
def test_real_embeddings_are_unit_length_and_do_not_depend_on_batching():
    enc = load_encoder("multilingual-e5-small-q8_0")
    texts = ["remind me to call asha", "kal subah meeting yaad dilana", "hello kestrel"]
    batch = run_llama_embedding(enc, [model_input(t, enc.prefix) for t in texts])
    assert batch.shape == (3, 384)
    assert np.allclose(np.linalg.norm(batch, axis=1), 1.0, atol=1e-5)
    # The Rust runtime embeds one request at a time; the cache embeds many per batch.
    alone = run_llama_embedding(enc, [model_input(texts[1], enc.prefix)])
    assert np.abs(alone[0] - batch[1]).max() < 1e-4
