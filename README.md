# quing, a (hopefully) minimalist toml based song shuffler

## Usage: quing [playlist.toml...]
```toml
name = "" # optional name of the playlist
time = -1 # optional setting for repeating a playlist n times. if the number is below zero, it'll repeat infinitely

[[song]]
name = "" # same as playlist-level name, for a song.
file = "" # file path pointing towards a file which contains audio data.
# supported features:
#  environment variables: ${NAME}
#  NOTE: redcursive variables also work: $${NAME} => ${VALUE_OF_NAME} => {VALUE_OF_VALUE_OF_NAME}
#  ~ as a shortcut for $HOME
#
# NOTE: it isn't suggested to use relative paths, for files.

time = -1 # similar to playlist-level time, but for a single song.
```

## Controls:
- #### quing supports simple playback controls (pausing and skipping)
```toml
'ctrl + c' = "exit the program when in active playback"
'ctrl + l' = "skip one playlist forwards"
'ctrl + j' = "skip one playlist backwards"
'l' = "skip one track forwards"
'j' = "skip one track backwards"
'k' = "toggle the playback"
'shift  + k' = "Toggle the volume of the currently playing track"
'shift  + l' = "Increase the volume of the currently playing track"
'shift + j' = "Decrease the volume of the currently playing track"
```

###### NOTE: all inputted playlists are loaded upon starting.
###### NOTE: all files of the playlist are loaded when it is its turn to play.
