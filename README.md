# quing, a (hopefully) minimalist toml based music player

## Usage: quing [-flags...] [playlist.toml...]
```toml
name = "" # optional name of the playlist
time = -1 # an optional setting for repeating a playlist n times. if the number is below zero, it'll repeat infinitely

[[song]]
name = "" # same as playlist-level name, though for a song.
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
'h' = "to manually enter headless mode."
'f' = "to merge all tracks, from the playlist files, into one."
'v' = "to output some general package information."
'p' = "repeat the composed file-playlist for ever."
't' = "repeat the inputted file, inside of the file-playlist, infinitely."
```

## Controls:
```toml
'n     ' = "skip one playlist forwards"
'p     ' = "skip one playlist backwards"
'ctrl_c' = "exit the program when in active playback"
'     l' = "skip one track forwards"
'     j' = "skip one track backwards"
'     k' = "toggle the playback"
'up    ' = "Increase the volume of the currently playing track"
'down  ' = "Decrease the volume of the currently playing track"
'm     ' = "Toggle the volume of the currently playing track"
```

###### NOTE: all inputted playlists are loaded upon starting. all files of the playlist are loaded when it is its turn to play. the program will run in headless mode if the creation of the control thread fails.
