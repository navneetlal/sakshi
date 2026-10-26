# nemo-speech output samples

Real output of `nemo-speech` 0.2.0 (macOS arm64, Metal), captured with the
command lines recorded in `DECISIONS.md` (D9). The parser tests run against
these files. Only the absolute paths in `"file"` and on the stderr progress
line were replaced with bare file names; nothing else was edited.

| File | Command | Audio source | License |
|---|---|---|---|
| `ami_system.json`, `ami_system.stderr.txt` | `system` track: transcribe + diarize | `ami_en2002d_2132.wav` from NeMo-Speech.cpp `test_files/diar/`: 60 s of AMI meeting EN2002d (Mix-Headset, from 2132.5 s) | AMI Meeting Corpus, [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) |
| `fleurs_hi_mic.json`, `fleurs_hi_mic.stderr.txt` | `mic` track: transcribe only | FLEURS `hi_in` dev utterance `5958463932227431376.wav` | Google FLEURS, [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) |

The transcripts are machine output derived from that audio and carry the
same attribution requirements.
