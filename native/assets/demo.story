// PLACEHOLDER: shows the script format only. Replace it with the real story.
@char Ruby color=#ff5a5a blip=square pitch=520 speed=1.0
@char Azure color=#5ac8ff blip=triangle pitch=330 speed=0.85
@char Narrator color=#c8c8dc blip=sine pitch=260

* The lanterns along the track are already lit.
Ruby: You came back. {p} I didn't think you would.
Azure: Neither did I.
Ruby: Then let's finish it. Whoever crosses first gets to ask for one thing.
@race map=neon seed=ritual-demo marbles=Ruby,Azure len=30 title=The Ritual
Azure: ...{p} {winner}. Of course it was {winner}.
* The lanterns go out, one by one.
