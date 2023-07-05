# quing, a (hopefully) minimalist toml based song shuffler

## Usage: quing [playlist.toml...]
```toml
# playlist.toml
# NOTE: does not need to be a toml file as long it is the toml format

name = "Foo" # name of the playlist

[[song]]
name = "Bar"              # name of an individual playlist entry
file = "$PATH/to/bar.mp3" # path to file, supports environment variables and '~'

[[song]]
name = "Baz"
file = "~/baz.wav"
```

## Controls:
- #### quing supports simple playback controls (pausing and skipping)
```
'q' or 'c': exit the program when in active playback
'/' or 'h': skip one playlist forwards
'.' or 'l': skip one track forwards
',' or 'j': skip one track backwards
' ' or 'k': pause the playback
```

###### NOTE: skipping is slightly expensive.
