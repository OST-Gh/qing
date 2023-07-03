///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::fs::{ self, File };
use nitrogen::{ fmt_path, traits::* };
use oxygen::*;
use serde::Deserialize;
use rodio::{
	OutputStream,
	Decoder,
	source::{
		Pausable,
		Skippable,
		Source,
	},
};
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
pub const LINE: &str = Formatting::UnderLined.enable();
pub const ENBOLD: &str = Formatting::Bold.enable();
pub const DISBOLD: &str = Formatting::Bold.disable();
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[derive(Deserialize)]
struct Songlist {
	name: Box<str>,
	song: Vec<Song>,
}

#[derive(Deserialize)]
struct Song {
	name: Box<str>,
	file: Box<str>,
}

struct Stream {
	name: Box<str>,
	file: Skippable<Pausable<Decoder<File>>>,
}

struct Streamlist {
	name: Box<str>,
	song: Vec<Stream>,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
fn load(files: impl Iterator<Item = impl AsRef<str>>) -> Vec<Streamlist> {
	let mut generator = fastrand::Rng::new();
	files
		.filter_map(|path|
			{
				let Songlist { name, mut song } = fs::read_to_string(fmt_path(path))
					.ok()?
					.pipe(|ref contents| toml::from_str(contents))
					.ok()?;
				let mut new = Vec::with_capacity(song.len());
				while !song.is_empty() {
					let song = song.swap_remove(generator.usize(0..song.len()));
					let file = File::open(fmt_path(song.file))
						.ok()?
						.pipe(Decoder::new)
						.ok()?
						.pausable(false)
						.skippable();
					new.push(Stream { name: song.name, file })
				}
				Some(Streamlist { name, song: new })
			}
		)
		.collect()
}

fn main() {
	let handle = custom![
		'\r',
		Time::from(' '),
		Colour::from(Empty)
			.colour(colours::QING)
			.terminated(false),
	]
		.pipe(Handle::from);

	for path in std::env::args().skip(1) {

		handle.print(format!("Loading and parsing data from [{path}]."));
		let Songlist { song, name } = match fs::read_to_string(fmt_path(&path)).map(|contents| toml::from_str(&contents)) {
			Ok(Ok(playlist)) => playlist,
			Ok(Err(why)) => {
				handle.print(format!("{LINE}An error occured whilst attempting to parse the contents of [{path}]; '{ENBOLD}{why}{DISBOLD}'"));
				continue
			},
			Err(why) => {
				handle.print(format!("{LINE}An error occured whilst attempting to load the contents of [{path}]; '{ENBOLD}{why}{DISBOLD}'"));
				continue
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
		let handles = match OutputStream::try_default() {
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
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
