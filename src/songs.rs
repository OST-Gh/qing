///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use serde::Deserialize;
use std::{
	sync::Once,
	thread::sleep,
	io::{ BufReader, Seek, Write },
	ops::{ Deref, DerefMut },
	fs::{ File, read_to_string },
};
use crate::{
	TICK,
	DISCONNECTED,
	Duration,
	Instant,
	Sink,
	RecvTimeoutError,
	log,
	fmt_path,
	stdout,
	in_out::{
		Bundle,
		Controls,
		Layer,
	},
};
use lofty::{ read_from_path, AudioFile };
use toml::from_str;
use fastrand::Rng;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Global audio stream data.
pub(crate) static mut FILES: Vec<MetaData> = Vec::new();
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(debug_assertions, derive(Debug))]
#[derive(Deserialize)]
/// A playlist with some metadata.
pub(crate) struct Playlist {
	name: Option<Box<str>>,
	song: Vec<Track>,
	time: Option<isize>,
}

#[cfg_attr(debug_assertions, derive(Debug))]
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
#[cfg_attr(debug_assertions, derive(Debug))]
/// An instruction that the [`play`] function of a [`Playlist`] uses to control itself.
///
/// [`play`]: Playlist::play
pub(crate) enum Instruction {
	/// Don't progress the playback index.
	Hold,
	/// The playback finished without any issues.
	Done,
	/// Manual skip to the next track.
	Next,
	/// Manual backwards skip to a previous track.
	Back,
	/// Exit the program.
	Exit,
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
	pub(crate) fn try_from_contents((contents, path): (String, String)) -> Option<Self> {
		match from_str(&contents) {
			Ok(playlist) => Some(playlist),
			Err(why) => log!(err[path]: "parsing [{path}]" => why; None?),
		}
	}

	pub(crate) fn from_outliers(iterator: impl Iterator<Item = String>) -> (Self, Vec<(String, String)>) {
		let mut rest = Vec::with_capacity(8);
		(
			iterator.fold(
				Playlist {
					name: None,
					song: Vec::with_capacity(8),
					time: None
				},
				|mut playlist, path|
					{

						match read_to_string(fmt_path(&path)) {
							Ok(contents) => rest.push((contents, path)),
							Err(why) => {
								log!(err[path]: "loading [{path}]" => why);
								let boxed = path.into_boxed_str();
								playlist
									.song
									.push(
										Track {
											name: Some(boxed.clone()),
											file: boxed,
											time: None,
										}
									);
							},
						}
						playlist
					}
			),
			rest
		)
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
						files.push(
							MetaData {
								stream: BufReader::new(contents),
								duration: info
									.properties()
									.duration(),
							}
						)
					);
				},
				(Err(why), Ok(_)) => log!(err[name]: "loading [{name}]" => why),
				(Ok(_), Err(why)) => log!(err[name]: "loading [{name}]" => why),
				(Err(file_why), Err(info_why)) => log!(err[name]: "loading [{name}]" => file_why info_why),
			}
		}
	}

	/// Play the entire list back.
	///
	/// # The Output's Meaning:
	/// - [`true`]: the program has been manually exited.
	/// - [`false`]: progress to the next playlist.
	pub(crate) fn play(&mut self, bundle: &Bundle, lists_index: &mut usize, volume: &mut f32) -> bool { // bool = should exit or not.
		let name = self.get_name();
		let Self { ref mut song, .. } = self;

		let old_lists_index = *lists_index;
		let songs_length = song.len();
		let mut songs_index = 0;

		let controls = bundle.get_controls();

		while songs_index < songs_length {
			let old_songs_index = songs_index; // (sort of) proxy to index (used because of rewind code)
			// unless something is very wrong with the index (old), this will not error.
			let song = unsafe {
				self
					.song
					.get_unchecked_mut(old_songs_index)
			};


			match bundle.play_file(get_file(old_songs_index)) {
				Ok(playback) => 'song: {
					let Some(controls) = controls else { break 'song song.play_headless(playback, &name, &mut songs_index, volume) };
					match song.play(playback, &name, &mut songs_index, controls, volume) {
						Instruction::Hold => { },
						Instruction::Done => self.repeat_or_increment(lists_index),
						Instruction::Next => {
							*lists_index += 1;
							return false
						},
						Instruction::Back => {
							*lists_index -= (old_lists_index > 0) as usize;
							return false
						},
						Instruction::Exit => return true,
					}
				},
				Err(why) => log!(err[name]: "playing [{name}]" => why; return true), // assume error will occur on the other tracks too
			}
			if let Err(why) = get_file(old_songs_index).rewind() { log!(err[name]: "rewinding [{name}]" => why) }
		}
		*lists_index -= (old_lists_index > 0) as usize;
		false
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

	/// Play without a head.
	///
	/// This will severily impare one's ability to control the playback.
	pub(crate) fn play_headless(&mut self, playback: Sink, playlist_name: &str, songs_index: &mut usize, volume: &mut f32)  {
		let name = self.get_name();
		playback.set_volume(volume.clamp(0.0, 2.0));

		let mut elapsed = Duration::ZERO;
		let duration = get_file(*songs_index).get_duration();

		while elapsed <= duration {
			print!("\r[{playlist_name}][{name}][{}][{volume:>5.2}]\0",
				{
					let seconds = elapsed.as_secs();
					let minutes = seconds / 60;
					format!("{:0>2}:{:0>2}:{:0>2}", minutes / 60, minutes % 60, seconds % 60)
				}
			);
			if let Err(why) = stdout().flush() { log!(err: "flushing" => why) }

			sleep(TICK);
			elapsed += TICK
		} 
		self.repeat_or_increment(songs_index);
	}

	/// Play the track back.
	pub(crate) fn play(&mut self, playback: Sink, playlist_name: &str, songs_index: &mut usize, controls: &Controls, volume: &mut f32) -> Instruction {
		let name = self.get_name();
		playback.set_volume(volume.clamp(0.0, 2.0));

		let duration = get_file(*songs_index).get_duration();
		let mut elapsed = Duration::ZERO;

		while elapsed <= duration {
			let paused = playback.is_paused();

			print!("\r[{playlist_name}][{name}][{}][{volume:>5.2}]\0",
				{
					let seconds = elapsed.as_secs();
					let minutes = seconds / 60;
					format!("{:0>2}:{:0>2}:{:0>2}", minutes / 60, minutes % 60, seconds % 60)
				}
			);
			if let Err(why) = stdout().flush() { log!(err: "flushing" => why) }

			let now = Instant::now();
			elapsed += match controls.receive_signal(now) {
				Err(RecvTimeoutError::Timeout) => if paused { continue } else { TICK },
				Ok(Layer::Playlist(signal)) => return signal.manage(elapsed),

				Ok(Layer::Track(signal)) => if signal.manage(&playback, elapsed, songs_index) { return Instruction::Hold } else { now.elapsed() },
				Ok(Layer::Volume(signal)) => signal.manage(&playback, now, volume),

				Err(RecvTimeoutError::Disconnected) => {
					log!(err: "receiving control-thread" => DISCONNECTED);
					return Instruction::Exit
				}, // chain reaction will follow
			}
		}
		self.repeat_or_increment(songs_index);
		Instruction::Done
	}
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
