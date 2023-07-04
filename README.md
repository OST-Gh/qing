# qing
- a (hopefully) minimalist toml based song shuffler

# Usage: qing [playlist.toml...]
# What a playlist.toml looks like (note that playlist.toml can be any file as long it is written in the toml format):
```toml
name = "Foo" # name of the playlist

[[song]]
name = "Bar" # name of an individual playlist entry
file = "$PATH/to/bar.mp3"

[[song]]
name = "Baz"
file = "~/baz.wav" # qing currently suppports wav and mp3 files
```
