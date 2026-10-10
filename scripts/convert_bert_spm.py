#!/usr/bin/env python3
"""Run llama.cpp's convert_hf_to_gguf.py with one local patch, for multilingual-e5-small.

Why: multilingual-e5-small's config.json says `"architectures": ["BertModel"]` (BERT
positions starting at 0) but its tokenizer is XLM-R SentencePiece (Unigram). The pinned
converter picks its class from the architecture, so it takes the BERT/WordPiece vocab path
and fails ("BPE pre-tokenizer was not recognized"). Forcing the XLM-R class is wrong too:
it drops position-embedding rows (pad_token_id + 1 offset) that a `bert` model uses.

Patch: BertModel.set_vocab uses the XLM-R SentencePiece vocab writer when tokenizer.json is
Unigram; tensors keep the plain BERT handling. Upstream source is not modified.
Correctness is checked by the Week-5 Python-vs-GGUF embedding parity test.

Usage: same arguments as convert_hf_to_gguf.py.
"""

import json
import sys
from pathlib import Path

LLAMA = Path(__file__).resolve().parent.parent / "third_party" / "llama.cpp"
sys.path.insert(0, str(LLAMA))

import convert_hf_to_gguf  # noqa: E402
from conversion.bert import BertModel  # noqa: E402

_wordpiece_set_vocab = BertModel.set_vocab


def set_vocab(self) -> None:
    tokenizer = json.loads((self.dir_model / "tokenizer.json").read_text())
    if tokenizer["model"]["type"] == "Unigram":
        self._xlmroberta_set_vocab()
    else:
        _wordpiece_set_vocab(self)


BertModel.set_vocab = set_vocab

if __name__ == "__main__":
    convert_hf_to_gguf.main()
