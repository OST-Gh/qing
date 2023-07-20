# quing, a (hopefully) minimalist toml based song shuffler

## Usage: quing [playlist.toml...]
```toml
name = "" # name of the playlist

[[song]]
name = "" # name of the song
file = "" # file path pointing towards a file which contains audio data.
# supported features:
#  environment variables: ${NAME}
#  ~ as a shortcut for $HOME
#
# NOTE: it isn't suggested to use relative paths, for files.

time = -1 # optional setting for repeating a song n times. if the number is negative, it'll repeat infinitely.
```

## Controls:
- #### quing supports simple playback controls (pausing and skipping)
```toml
c = "exit the program when in active playback"
n = "skip one playlist forwards"
l = "skip one track forwards"
j = "skip one track backwards"
k = "pause or start the playback"
```

###### NOTE: all files are loaded upon startup, meaning that there's a (os set) hard cap on playlist length.

### Other:
> The current implementation of the controls is causing a lot of idle-wake-ups on MacOS which i'd like to minimise.
> Another problem is that whenever this program runs, it sets the current terminal's mode into raw which only gets turned off upon exiting.
> What i am trying to say is that when the program doesn't exit naturally it'll cause some problems for the current terminal.
>
> Another thing i've recently noticed that it is impacting the cpu quite the bit and i'd also like to minimize that.
> If anyone knows how to do so, please do open an issua or a pull-request on quing's github.
