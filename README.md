# quing, a (hopefully) minimalist toml based music player

## Usage: quing [-flags...] [playlist.toml...]
```toml
time = -1 # an optional setting for repeating a playlist n times. if the number is below zero, it'll repeat infinitely

[[song]]
file = "" # file path pointing towards a file, which contains audio data.
# supported features:
#  environment variables: ${NAME}
#  NOTE: redcursive variables do also work e.g.: $${NAME} => ${VALUE_OF_NAME} => {VALUE_OF_VALUE_OF_NAME}
#  ~, at the start of the path, as a shortcut, for $HOME.

time = -1 # similar to playlist-level time, but for a single song.
```

## Flags:
#### All flags must be passed in before the playlist files and start with a dash ('-').
```toml
'n' = "to not shuffle every playlist"
'f' = "to merge all tracks, from the playlist files, into one."
'v' = "to output some general package information."
'p' = "repeat the composed file-playlist for ever."
't' = "repeat the inputted file, inside of the file-playlist, infinitely."
```

## Controls:
```toml
'C-l' = "skip one playlist forwards"
'C-j' = "skip one playlist backwards"
'C-k' = "exit the program when in active playback"
'C-h' = "reset back to the first playlist"
'  l' = "skip one track forwards"
'  j' = "skip one track backwards"
'  k' = "toggle the playback"
'  h' = "reset back to the first track"
'S-l' = "increase the volume"
'S-j' = "decrease the volume"
'S-k' = "toggle the volume"
'S-h' = "reset the volume"
```

Due to the nature of the updated control code, the program will often need a second input before fully shutting down.
