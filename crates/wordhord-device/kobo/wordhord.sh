#!/bin/sh
# Starts the Wordhord reader from NickelMenu. Nickel is paused, not
# killed: when the reader exits, however it exits, Nickel resumes where it
# was, with its screen put back.

DIR=/mnt/onboard/.adds/wordhord
LOG="$DIR/wordhord.log"
SCREEN=/tmp/wordhord-screen
PAUSED="nickel sickel hindenburg fickel strickel fontickel"

cd "$DIR" || exit 1
if [ -e "$LOG" ] && [ "$(stat -c %s "$LOG")" -gt 262144 ]; then
	mv -f "$LOG" "$LOG.old"
fi
echo "--- $(date)" >>"$LOG"

sync
# shellcheck disable=SC2086
killall -q -STOP $PAUSED

./wordhord-reader "$DIR/library" "$DIR" "$SCREEN" >>"$LOG" 2>&1
status=$?
echo "exit $status" >>"$LOG"
if [ "$status" -ne 0 ]; then
	./wordhord-reader --restore "$SCREEN" >>"$LOG" 2>&1
fi
rm -f "$SCREEN"

# shellcheck disable=SC2086
killall -q -CONT $PAUSED
