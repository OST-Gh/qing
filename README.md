# qing, a (hopefully) minimalist toml based song shuffler

## Usage: qing [playlist.toml...]
```toml
# qing currently suppports wav and mp3 files

# Playlist.toml
# NOTE: does not need to be a toml file as long it is the toml format

name = "Foo" # name of the playlist

[[song]]
name = "Bar"              # name of an individual playlist entry
file = "$PATH/to/bar.mp3" # path to file, supports environment variables and '~'

[[song]]
name = "Baz"
file = "~/baz.wav"
```
