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

### Other:
> The current implementation of the controls is causing a lot of idle-wake-ups on MacOS which i'd like to minimise.
> Another problem is that whenever this program runs, it sets the current terminal's mode into raw which only gets turned off upon exiting.
> What i am trying to say is that when the program doesn't exit naturally it'll cause some problems for the current terminal.
