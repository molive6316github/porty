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

## Cover mode: voice changer (kNN-VC, fully local)
`marbeat/ai/vc/` converts a vocals-only recording into each marble's voice, so timing and melody come from the real singing.
- `worker.js`: our implementation of [kNN-VC](https://github.com/bshall/knn-vc) (Baas et al., Interspeech 2023, MIT): WavLM-Large layer-6 features,
  k=4 nearest-neighbour matching against the target voice's sample, HiFi-GAN vocoder (outputs 256 samples/frame, stretched x1.25 to restore timing).
- `wavlm_mm8.part00/01`: WavLM-Large (Microsoft, MIT) layers 0-6, ONNX from [TigreGotico/voiceclonnx-knn-vc](https://huggingface.co/TigreGotico/voiceclonnx-knn-vc),
  **re-quantized by us (MatMul-only int8)** because the original int8 file uses ConvInteger, which onnxruntime-web can't run. Split into two files for GitHub's 100 MB limit; the page rejoins them.
  Feature parity with the fp32 original: cosine similarity 0.9986.
- `hifigan_knnvc.onnx`: prematched HiFi-GAN vocoder (fp32).
- onnxruntime-web files are shared with `ai/clone/`.

Needs the acapella (vocals-only) of the same song, same length. Only convert voices you have the right to use.

## Automatic vocal separation (Spleeter, fully local)
Cover mode can make the acapella itself. `marbeat/ai/sep/` runs [Spleeter](https://github.com/deezer/spleeter) (Deezer, MIT), 2-stem vocals model,
using the ONNX export from [csukuangfj/sherpa-onnx-spleeter-2stems](https://huggingface.co/csukuangfj/sherpa-onnx-spleeter-2stems) (fp32, `vocals.onnx`).
`worker.js` does the STFT (4096/1024), runs the model on the section of the song you picked, builds a soft mask, and inverts the STFT.
Instrumental = mix - vocals, so the two always sum to the original. A real acapella upload still sounds better.

## Preview vs Render
- **Preview** plays live in the page (fast settings). 
- **Render** (`🎬 Render`) builds the final video offline, frame by frame, so it looks and sounds the same on any computer:
  voices re-made with slower, cleaner settings (Piper noise 0.2, Pocket TTS 4 flow steps), audio mixed with `OfflineAudioContext`
  through a voice bus (EQ, presence, gentle compression, short room reverb) and a master stage (-14 dBFS RMS, soft limiter),
  frames drawn at up to 1080x1920 / 60 fps with motion blur (3 sub-frames), glow, comet trails, camera shake and vignette,
  encoded with WebCodecs (H.264 + AAC if the browser can, else H.264 + Opus or VP9 + Opus) and muxed with
  [mp4-muxer](https://github.com/Vanilagy/mp4-muxer) / [webm-muxer](https://github.com/Vanilagy/webm-muxer) (MIT, bundled in `ai/render/muxers.js`).

## Render: the "pro" look and sound
- **Look:** glass marbles (sprite with rim light, reflection, specular, soft shadow), neon-tube ramps, glowing pegs, bumpers with
  rings that pulse on the beat, parallax background with map-themed ambience (stars, embers, snow, bubbles, sparkles), comet trails,
  crown + name tag on the leader, a flash ring on every overtake, glass HUD with animated equalizer, karaoke-style captions
  (previous / current / next word, pop-in), optional title card, confetti + winner card + slow zoom at the finish,
  bloom, colour grade, vignette and film grain. Up to 1080x1920 / 60 fps with 4-step motion blur.
- **Sound:** voice bus (warmth, EQ, presence, light compression, stereo doubling, tempo-synced echo, room), synthesised sound effects
  from the real physics collisions (pegs, bumpers, ramps, marble-on-marble), a whoosh on every overtake and a finish stinger + cheer,
  then mastering to **-14 LUFS** (ITU BS.1770 K-weighted, gated) with a lookahead limiter (peaks stay under about -1.3 dBFS).

## Windows app and updates
`download/Marbeat.exe` is a launcher that serves this same web app from a local cache and updates itself from `manifest.json`.
See `launcher/README.md` (in the main repo). After changing anything here, run `tools/build-launcher.sh` so `manifest.json` stays in sync.
