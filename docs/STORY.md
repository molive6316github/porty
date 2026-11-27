# Marbeat story: design notes

Locked in
- Marble race software with neon tracks, a live leaderboard, and tournaments.
- A real story on top, told with marbles.
- Tone: edge of romance, slight fantasy, competitive, loss, personal.
- Dialogue: Undertale style. Typewriter text, a different blip per character.
- The race is a ritual with something at the end. It is not a league.

Dropped
- "Only the leader gets a voice." Singing may go entirely; it stays as an off-by-default setting.
- The kiddie, cartoon version.

Engine (native/core/src/story.rs, native/app Story tab)
- Plain-text scripts, see native/assets/demo.story (a placeholder that only shows the format).
- `@char` declares a speaker (colour, blip waveform, pitch, speed). `Name: text` speaks. `* text` narrates.
- `@race` runs a real race as a story beat (map, seed, marbles, rigged winner, length, title). `{winner}` carries into later lines.
- `{p}` is a short pause. `@wait N` is a timed pause.
- Space, Enter, Z or a click advances. A race cannot be skipped.

Open questions
- What is at the end of the ritual, and what does each marble want?
- Music: does the song drive the race (beats, length, finish on the last beat) or sit under it?
