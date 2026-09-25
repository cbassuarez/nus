#!/usr/bin/env python3
"""The pane director on the real app: every layout op, and its undo and redo.

Uses the same temporary-profile runner as check-settings.py:
    scripts/check-panes.py [path/to/nus.app]
"""
from pathlib import Path
exec(compile(Path(__file__).with_name('check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))

prefs={'behavior':{'splash':'None','update_checks':False},'motion':{'register':0.5,'reduce':True},'window_rect':[80,80,1440,1000]}
steps='''
newshell
wait 300
assertlayout split=no left=term
# Split, and undo takes the new pane away; redo brings one back.
pane split
wait 300
assertlayout split=yes left=term right=web focus=right
pane undo
assertlayout split=no
pane redo
wait 300
assertlayout split=yes right=web
# Swap and its undo.
pane swap
assertlayout left=web right=term
pane undo
assertlayout left=term right=web
# Solo, both, and undo of each.
pane solo left
assertlayout solo=yes focus=left
pane both
assertlayout solo=no
pane undo
assertlayout solo=yes focus=left
pane undo
assertlayout solo=no
# Width, and undo back to the default.
pane width 420
assertlayout width=420
pane undo
assertlayout width=default
# To a tab, and undo joins it back where it was.
asserttabs 2
pane totab right
assertlayout split=no left=web
asserttabs 3
pane undo
asserttabs 2
assertlayout split=yes left=term right=web
pane redo
asserttabs 3
pane undo
# Kill the right pane: no way back from a stopped process, but a split undoes.
pane kill right
assertlayout split=no left=term
shot panes
'''
run('panes', steps, prefs)

# The tiling is a tree: an L from three, a fourth splits the largest tile,
# every step undoes.
tiles='''
newshell
newshell
newshell
wait 400
asserttabs 4
tile 1 2 3
assertlayout tiled=3
asserttileshape L
tile 0
assertlayout tiled=4
pane undo
assertlayout tiled=3
asserttileshape L
split
assertlayout tiled=0
pane undo
assertlayout tiled=3
shot tiles
'''
run('tiles', tiles, prefs)

# Drop zones and pane mode. Tabs dragged onto the page tile beside what it
# shows; pane mode walks, zooms and evens them by key; one undo takes a
# whole drop back.
modes='''
newshell
newshell
newshell
wait 400
asserttabs 4
assertlayout active=3
droptab 1 0.9 0.5
assertlayout tiled=2 active=1
droptab 2 0.25 0.95
assertlayout tiled=3 active=2
asserttileshape L
pane undo
assertlayout tiled=2
pane redo
assertlayout tiled=3
key ctrl+alt+p
assertlayout mode=on
key l
assertlayout active=1
key h
assertlayout active=3
key z
assertlayout tiled=0
key z
assertlayout tiled=3
key right
key =
asserttileshape L
key u
key u
asserttileshape L
shot pane-mode
key esc
assertlayout mode=off
# A split tab's pane dragged to the bottom edge of the other: its own tab,
# tiled under (the window is wide enough that a split shows both panes).
newshell
wait 300
pane split
wait 300
assertlayout split=yes
droppane right 0.25 0.95
assertlayout split=no tiled=2
pane undo
assertlayout split=yes tiled=0
shot drops
'''
wide=dict(prefs, window_rect=[40,40,2800,1800])
run('pane-mode', modes, wide)
# Between windows: a shell sent to a new window arrives alive (what it
# printed is still on its screen) and the new window's own first tab gives
# way; a shell sent back with pane mode's `w` arrives in the first window.
first='''
newshell
wait 400
shell echo MOVED-$((6*7))
wait 400
assertshell MOVED-42
asserttabs 2
send new
wait 3000
asserttabs 1
assertwindows 2
wait 5000
asserttabs 2
assertlayout left=term
shot back-home
'''
second='''
wait 2500
asserttabs 1
assertshell MOVED-42
newshell
wait 600
asserttabs 2
key ctrl+alt+p
key w
wait 400
asserttabs 1
assertshell MOVED-42
shot arrived
# The first window finishes first: its script ending is the run's end.
wait 20000
'''
run('windows', first, prefs, second=second)

# Layout files keep the tiling: three shells in an L, saved, untiled; the
# file opened again makes three more tabs, tiled the same way.
layouts='''
newshell
newshell
newshell
wait 400
tile 1 2 3
asserttileshape L
savelayout trio
split
assertlayout tiled=0
asserttabs 4
openlayout trio
wait 800
asserttabs 7
asserttileshape L
shot layout-reopened
'''
run('layouts', layouts, prefs)
print('Pane director checks passed.', flush=True)
