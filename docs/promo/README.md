# promo shots

The scripts the film's footage is recorded with. Each one runs on nus's
own recorder (`spikes/composite/src/shot.rs`, `clock.rs`), so every take
is frames on a fixed clock rather than a screen capture.

Run one from a built bundle (CEF needs the bundle on macOS):

```sh
NUS_SHOT=docs/promo/shell-min.shot \
NUS_SHOT_SIZE=1600x1000 \
NUS_SHOT_OUT=~/footage \
  dist/nus.app/Contents/MacOS/nus
```

`NUS_SHOT_SIZE` fixes the window at creation, so on a 2x display every
capture is 3200x2000. `NUS_SHOT_DIR` points the profile somewhere else,
which is how a clean staging is kept apart from your own.

Then:

```sh
ffmpeg -framerate 60 -i f%05d.png -c:v libx264 -preset slow -crf 10 \
  -pix_fmt yuv420p shell-min.mp4
```

The PNG sequence is the master. The mp4 is for watching.

House rules for a take, in full in `nus-promo/RESHOOT.md`: settle before
you record, one idea per clip, nothing personal in frame, and two clean
takes of anything the film leans on.
