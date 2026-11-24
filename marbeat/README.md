# Marbeat

Song → marble race video. Open `index.html` in Chrome (needs `mespeak.bundle.js` next to it).

- **Sing lyrics** engine: type lyrics, one word lands per detected beat; the leading marble sings it with a
  synthesized character voice. Pitch loosely follows the song melody.
- **Remix the song** engine: pitch-shifts the song itself per marble.
- Attach your own audio file to a marble to use it instead of a synth voice.

## Licensing
`mespeak.bundle.js` bundles [meSpeak](https://www.masswerk.at/mespeak/) / eSpeak, which are **GPL**.
If you distribute Marbeat, the combined work must comply with the GPL. It was built from the `mespeak` npm
package with a one-line patch exposing `recycle()` (works around its known "breaks every ~80th call" bug).
The character voices are original archetypes, not clones of any real person's or character's voice.

## AI sync (Whisper, fully local)
`marbeat/ai/` holds everything the AI lyric sync needs, so no CDN or outside server is contacted:
- `transformers.min.js` (transformers.js 2.17.2, Apache-2.0) and `ort-wasm*.wasm` (onnxruntime-web 1.14, MIT)
- `models/Xenova/whisper-tiny.en/` (OpenAI Whisper tiny.en, MIT; quantized ONNX export, ~41 MB)

It must be loaded over http(s) (e.g. the Vercel deploy), not by double-clicking `index.html`, because browsers
block loading model files from `file://`.

## Neural voices (Piper, fully local)
`marbeat/ai/piper/` holds a local neural text-to-speech stack, so no CDN is used:
- `en_GB-vctk-medium.onnx` (+ `.json`): Piper VITS voice, 109 speakers (CSTR VCTK corpus, CC BY 4.0), ~77 MB
- `piper-phonemize.mjs`, `piper_phonemize.wasm/.data`: espeak-ng phonemizer (GPL)
- `ort.wasm.min.js`, `ort-wasm*.wasm`: onnxruntime-web 1.18 (MIT)
- `piper.js`: small loader

Each word is synthesized in a character's speaker voice, time-stretched (WSOLA) to fit its slot, retuned to the
song's melody, given light vibrato on held notes, then run through the character effect. Only the words a marble
actually sings (while it leads) are synthesized. "Robot voices" (eSpeak) remains as a light fallback.

## Voice cloning (Pocket TTS, fully local)
`marbeat/ai/clone/` runs Kyutai's Pocket TTS (100M parameters, zero-shot voice cloning) in a Web Worker:
- `worker.js`: adapted from [KevinAHM/pocket-tts-web](https://huggingface.co/spaces/KevinAHM/pocket-tts-web) (Apache-2.0, see `CODE-LICENSE`).
  Changes: ONNX runtime loaded locally (no CDN), no bundled voices, several named cloned voices at once.
- `onnx/english_2026-04/`: INT8 ONNX export of [kyutai/pocket-tts](https://huggingface.co/kyutai/pocket-tts)
  by [KevinAHM](https://huggingface.co/KevinAHM/pocket-tts-onnx). Model licensed **CC-BY-4.0**; credit: Kyutai (model), KevinAHM (ONNX export).
- `sentencepiece.js` (tokenizer), `ort.min.mjs` + `ort-wasm-simd-threaded.*` (onnxruntime-web 1.20, MIT).

Only clone voices you have the right to use. Quality varies with the sample: use clear, dry speech, 5-15 s.
