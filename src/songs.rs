///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use serde::Deserialize;
use std::{
	io::BufReader,
	ops::{ Deref, DerefMut },
	fs::{ File, read_to_string },
};
use super::{
	Duration,
	log,
	fmt_path,
	clear,
};
use lofty::{ read_from_path, AudioFile };
use toml::from_str;
use fastrand::Rng;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Global audio stream data.
pub(crate) static mut FILES: Vec<MetaData> = Vec::new();
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[derive(Deserialize)]
/// A playlist with some metadata
pub(crate) struct Playlist {
	name: Option<Box<str>>,
	song: Vec<Track>,
	time: Option<isize>,
}

#[derive(Deserialize)]
#[derive(Clone)]
/// A song path with aditional metadata
pub(crate) struct Track {
	name: Option<Box<str>>,
	file: Box<str>,
	time: Option<isize>,
}

/// A Track's importand information.
pub(crate) struct MetaData {
	stream: BufReader<File>,
	duration: Duration,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
fn decrement_or_increment(decremented: &mut Option<isize>, incremented: &mut usize) {
	let mut new_decremented = decremented.unwrap_or_default();
	if new_decremented == 0 { *incremented += 1 } else {
		new_decremented -= 1;
		decremented
			.as_mut()
			.map(|inner_decremented| *inner_decremented = new_decremented);
	}
}

/// Apply a function to the files (mutable reference).
///
/// # Panics:
///
/// - If the function being executed panics.
pub(crate) fn map_files_mut<O>(function: impl FnOnce(&mut Vec<MetaData>) -> O) -> O {
	unsafe { function(&mut FILES) }
}

/// Get the file at the given index.
///
/// # Fails:
///
/// - The function does not panic, but it does not guarrante that the index is inside the bounds of the static.
pub(crate) fn get_file(index: usize) -> &'static mut MetaData {
	unsafe { FILES.get_unchecked_mut(index) }
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl Playlist {
	/// Load a playlist from a path.
	pub(crate) fn try_from_path(path: String) -> Option<Self> {
		log!(info[path]: "Loading and parsing data from [{path}].");
		match read_to_string(fmt_path(&path)).map(|contents| from_str(&contents)) {
			Ok(Ok(playlist)) => Some(playlist),
			Ok(Err(why)) => log!(err[path]: "parse the contents of [{path}]" => why; None?),
			Err(why) => log!(err[path]: "load the contents of [{path}]" => why; None?),
		}
	}

	/// Flatten the collective Playlists into a single Playlist
	pub(crate) fn flatten(lists: Vec<Self>) -> Self {
		let mut new_name = Vec::with_capacity(lists.len());

		let repeats = {
			let iterator = lists.iter();
			let minimum = iterator
				.clone()
				.min_by_key(|list|
						list
							.time
							.unwrap_or_default()
				)
				.expect("search for an infinity repeation  Empty Vector")
				.time
				.unwrap_or_default();
			if minimum < 0 { minimum } else {
				iterator
					.max_by_key(|list|
						list
							.time
							.unwrap_or_default()
					)
					.expect("search for the highest repeat count  Empty Vector")
					.time
					.unwrap_or_default()
			}
		};

		let tracks: Vec<Track> = lists
			.into_iter()
			.map(|list|
				{
					let name = String::from(list.get_name());
					new_name.push(name);
					list
						.song
						.into_iter()
				}
			)
			.flatten()
			.collect();

		Playlist {
			name: Some(
				new_name
					.join(" & ")
					.into_boxed_str()
			),
			song: tracks,
			time: Some(repeats),
		}
	}

	pub(crate) fn get_name(&self) -> &str {
		let Some(ref name) = self.name else { return "Untitled" };
		name
	}

	pub(crate) fn get_song_mut(&mut self) -> &mut [Track] { &mut self.song }

	pub(crate) fn shuffle_song(&mut self) {
		let name = self.get_name();
		log!(info[name]: "Shuffling all of the songs in [{name}].");
		let mut generator = Rng::new();

		let songs = &mut self.song;
		let length = songs.len();

		for value in 0..length {
			let index = value % length;
			songs.swap(index, generator.usize(0..=index));
			songs.swap(index, generator.usize(index..length));
			// a b c; b inclusive in both random ranges
			// b a c
			// b c a
		}

	}

	pub(crate) fn repeat_or_increment(&mut self, index: &mut usize) {
		decrement_or_increment(&mut self.time, index);
		map_files_mut(Vec::clear);
		clear()
	}

	pub(crate) fn load_song(&self) {
		let Self { song, .. } = self;
		let name = self.get_name();

		log!(info[name]: "Loading all of the audio contents of the songs in [{name}].");
		for Track { name, file, .. } in song.iter() {
			let name = name
				.clone()
				.unwrap_or_default();
			let formatted = fmt_path(file);
			match (File::open(&formatted), read_from_path(formatted)) {
				(Ok(contents), Ok(info)) => {
					map_files_mut(|files|
						files
							.push(
								MetaData {
									stream: BufReader::new(contents),
									duration: info
										.properties()
										.duration(),
								}
							)
					);
				},
				(Err(why), Ok(_)) => log!(err[name]: "load the audio contents of [{name}]" => why),
				(Ok(_), Err(why)) => log!(err[name]: "load the audio properties of [{name}]" => why),
				(Err(file_why), Err(info_why)) => log!(err[name]: "load the audio contents and properties of [{name}]" => file_why info_why),
			}
		}
		print!("\r\n\0");
	}
}

impl Track {
	pub(crate) fn get_name(&self) -> String {
		String::from(
			self
				.name
				.as_ref()
				.map_or("Untitled", |name| name)
		)
	}
	

	pub(crate) fn repeat_or_increment(&mut self, index: &mut usize) { decrement_or_increment(&mut self.time, index) }
}

impl MetaData {
	pub(crate) fn get_duration(&self) -> Duration { self.duration }
}

impl Deref for MetaData {
	type Target = BufReader<File>;

	fn deref(&self) -> &Self::Target { &self.stream }
}

impl DerefMut for MetaData {
	fn deref_mut(&mut self) -> &mut Self::Target { &mut self.stream }
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
