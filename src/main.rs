///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::fs::{ self, File };
use nitrogen::{ fmt_path, traits::* };
use oxygen::*;
use serde::Deserialize;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
pub const LINE: &str = Formatting::UnderLined.enable();
pub const ENBOLD: &str = Formatting::Bold.enable();
pub const DISBOLD: &str = Formatting::Bold.disable();
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[derive(Deserialize)]
struct Playlist {
	name: Box<str>,
	song: Vec<Song>,
}

#[derive(Deserialize)]
struct Song {
	name: Box<str>,
	file: Box<str>,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
fn main() {
	let handle = custom![
		Time::from(' '),
		Colour::from(Empty)
			.colour(colours::QING)
			.terminated(false),
	]
		.pipe(Handle::from);
	let Some(path) = std::env::args().nth(1) else { return };

	handle.print(format!("Loading and parsing data from [{path}]."));
	let Playlist { song, name } = match fs::read_to_string(fmt_path(&path)).map(|contents| toml::from_str(&contents)) {
		Ok(Ok(playlist)) => playlist,
		Ok(Err(why)) => {
			handle.print(format!("{LINE}A fatal error occured whilst attempting to parse the contents of [{path}]; '{ENBOLD}{why}{DISBOLD}'"));
			return
		},
		Err(why) => {
			handle.print(format!("{LINE}A fatal error occured whilst attempting to load the contents [{path}]; '{ENBOLD}{why}{DISBOLD}'"));
			return
		},
	};

	handle.print(format!("Loading all of the songs in [{name}]."));
	let mut files: Vec<(Box<str>, File)> = song
		.into_iter()
		.filter_map(|Song { name, file }|
			{
				handle.print(format!("Loading the audio contents of [{name}]."));
				match File::open(fmt_path(file)) {
					Ok(contents) => Some((name, contents)),
					Err(why) => {
						handle.print(format!("{LINE}An error occured whilst attempting to load the audio contents of [{name}]; '{ENBOLD}{why}{DISBOLD}'"));
						None
					},
				}
			}
		)
		.collect();
	handle.print('\0');

	let mut generator = fastrand::Rng::new();

	handle.print("Determining the output device.");
	let handles = match rodio::OutputStream::try_default() {
		Ok(handles) => handles,
		Err(why) => {
			handle.print(format!("{LINE}A fatal error occured whilst attempting to determine the default audio output device; '{ENBOLD}{why}{DISBOLD}'"));
			return
		},
	};

	handle.print(format!("Playing back all of the songs in [{name}]."));
	while !files.is_empty() {
		let (name, contents) = files.remove(generator.usize(0..files.len()));
		match handles
			.1
			.play_once(contents)
		{
			Ok(playback) => {
				handle.print(format!("Playing back the audio contents of [{name}]."));
				playback.sleep_until_end();
			},
			Err(why) => {
				handle.print(format!("{LINE}An error occured whilst attempting to playback [{name}] from the default audio output device; '{ENBOLD}{why}{DISBOLD}'"));
				continue
			},
		}
	}
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
