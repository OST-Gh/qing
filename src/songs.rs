///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use serde::Deserialize;
use std::{
	sync::Once,
	io::BufReader,
	ops::{ Deref, DerefMut },
	fs::{ File, read_to_string },
};
use super::{
	Duration,
	log,
	fmt_path,
};
use lofty::{ read_from_path, AudioFile };
use toml::from_str;
use fastrand::Rng;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Global audio stream data.
pub(crate) static mut FILES: Vec<MetaData> = Vec::new();
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[derive(Deserialize)]
/// A playlist with some metadata.
pub(crate) struct Playlist {
	name: Option<Box<str>>,
	song: Vec<Track>,
	time: Option<isize>,
}

#[derive(Deserialize)]
#[derive(Clone)]
/// A song path with aditional metadata.
pub(crate) struct Track {
	name: Option<Box<str>>,
	file: Box<str>,
	time: Option<isize>,
}

/// A Track's importand information.
pub(crate) struct MetaData {
	stream: BufReader<File>,
	/// The exact [`Duration`] of a [`stream`]
	///
	/// [`stream`]: Self#field.stream
	duration: Duration,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Utility function for implementing repetition behavior.
fn decrement_or_increment(decremented: &mut Option<isize>, incremented: &mut usize) {
	let mut new_decremented = decremented.unwrap_or_default();
	if new_decremented == 0 { *incremented += 1 } else {
		new_decremented -= 1;
		decremented
			.as_mut()
			.map(|inner_decremented| *inner_decremented = new_decremented);
	}
}

/// Implementation utility function for getting a [`Track`]'s or [`Playlist`]'s name.
fn name_from(optional: &Option<Box<str>>) -> String {
	String::from(
		optional
			.as_ref()
			.map_or("Untitled", |name| name)
	)
}

/// Apply a function to a mutable reference of [`FILES`].
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
/// - The function does not panic, but it does not guarrante that the index is inside the bounds of the global variable ([`FILES`]).
pub(crate) fn get_file(index: usize) -> &'static mut MetaData {
	unsafe { FILES.get_unchecked_mut(index) }
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl Playlist {
	/// Load a [`Playlist`] from a [`Path`] represented as a [`String`].
	///
	/// The string is, before being loaded, passed into the [`fmt_path`] function.
	///
	/// [`Path`]: std::path::Path
	pub(crate) fn try_from_path(path: String) -> Option<Self> {
		match read_to_string(fmt_path(&path)).map(|contents| from_str(&contents)) {
			Ok(Ok(playlist)) => Some(playlist),
			Ok(Err(why)) => log!(err[path]: "parse the contents of [{path}]" => why; None?),
			Err(why) => log!(err[path]: "load the contents of [{path}]" => why; None?),
		}
	}

	/// Merge a list of [`Playlists`] into a single [`Playlist`].
	///
	/// [`Playlists`]: Playlist
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

		Self {
			name: Some(
				new_name
					.join(" & ")
					.into_boxed_str()
			),
			song: tracks,
			time: Some(repeats),
		}
	}

	/// Get the name of the passed in [`Playlist`].
	///
	/// If the playlist's name is set to [`None`], the function will return the [`string slice`] `"Untitled"`.
	///
	/// [`string slice`]: str
	pub(crate) fn get_name(&self) -> String { name_from(&self.name) }

	/// Get access to a mutable reference of a slice containing [`Tracks`].
	///
	/// [`Tracks`]: Track
	pub(crate) fn get_song_mut(&mut self) -> &mut [Track] { &mut self.song }

	/// Perform an in-place item shuffle on the [`Playlist`]'s [`Tracks`].
	///
	/// [`Tracks`]: Track
	pub(crate) fn shuffle_song(&mut self) {
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

	/// Used for index based [`Playlist`] playback.
	///
	/// The function should be used so as to advance the playback.
	pub(crate) fn repeat_or_increment(&mut self, index: &mut usize) {
		decrement_or_increment(&mut self.time, index);
	}

	/// Load each [`Track`]'s duration and stream.
	///
	/// The function's output will be put into the global variable [`FILES`].\
	/// This function also clears [`FILES`] when it successfully loads at least one [`Track`].
	pub(crate) fn load_song(&self) {
		let Self { song, .. } = self;

		let startup_clear = Once::new();

		for Track { name, file, .. } in song.iter() {
			let name = name
				.clone()
				.unwrap_or_default();
			let formatted = fmt_path(file);

			match (File::open(&formatted), read_from_path(formatted)) {
				(Ok(contents), Ok(info)) => {
					startup_clear.call_once(|| map_files_mut(Vec::clear));
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
	}
}

impl Track {
	/// Get the name of the passed in [`Track`].
	///
	/// If the playlist's name is set to [`None`], the function will return the [`string slice`] `"Untitled"`.
	///
	/// [`string slice`]: str
	pub(crate) fn get_name(&self) -> String { name_from(&self.name) }

	/// Used for index based [`Track`] playback.
	///
	/// The function should be used so as to advance the playback.
	pub(crate) fn repeat_or_increment(&mut self, index: &mut usize) { decrement_or_increment(&mut self.time, index) }
}

impl MetaData {
	/// Copy the underlying [`Duration`] of the held [`stream`]
	///
	/// [`stream`]: Self#field.stream
	pub(crate) fn get_duration(&self) -> Duration { self.duration }
}

impl Deref for MetaData {
	type Target = BufReader<File>;

	/// Simply returns a reference to the [`stream`].
	///
	/// [`stream`]: Self#field.stream
	fn deref(&self) -> &Self::Target { &self.stream }
}

impl DerefMut for MetaData {
	/// Returns a mutable reference to the [`stream`].
	///
	/// [`stream`]: Self#field.stream
	fn deref_mut(&mut self) -> &mut Self::Target { &mut self.stream }
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
