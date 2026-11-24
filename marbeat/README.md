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
