///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use serde::Deserialize;
use std::fs::read_to_string;
use super::{
	Error,
	VectorError,
	utilities::fmt_path,
};
use toml::from_str;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// /// Global audio stream data.
// pub(crate) static mut FILES: Vec<MetaData> = Vec::new();

// static mut VOLUME: f32 = 1.0;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(debug_assertions, derive(Debug))]
#[derive(Deserialize)]
/// A playlist with some metadata.
pub struct Playlist {
	pub(crate) song: Vec<Track>,
	pub(crate) time: Option<isize>,
}

#[cfg_attr(debug_assertions, derive(Debug))]
#[derive(Deserialize)]
#[derive(Clone)]
/// A song path with aditional metadata.
pub struct Track {
	pub(crate) file: Box<str>,
	pub(crate) time: Option<isize>,
}

// /// A Track's importand information.
// pub(crate) struct MetaData {
// 	stream: BufReader<File>,
// 	/// The exact [`Duration`] of a [`stream`]
// 	///
// 	/// [`stream`]: Self#field.stream
// 	duration: Duration,
// }
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
// /// Apply a function to a mutable reference of [`FILES`].
// ///
// /// # Panics:
// ///
// /// - If the function being executed panics.
// pub(crate) fn map_files_mut<O>(function: impl FnOnce(&mut Vec<MetaData>) -> O) -> O {
// 	unsafe { function(&mut FILES) }
// }

// /// Get the file at the given index.
// ///
// /// # Fails:
// ///
// /// - The function does not panic, but it does not guarrante that the index is inside the bounds of the global variable ([`FILES`]).
// pub(crate) fn get_file(index: usize) -> &'static mut MetaData {
// 	unsafe { FILES.get_unchecked_mut(index) }
// }
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl Playlist {
	#[inline] pub fn song_get(&self) -> &Vec<Track> { &self.song }
	#[inline] pub fn song_get_mut(&mut self) -> &mut Vec<Track> { &mut self.song }
	#[inline] pub fn song_take(self) -> Vec<Track> { self.song }
	pub fn time_set(&mut self, value: isize) { self.time = Some(value) }
	pub fn time_unset(&mut self) { self.time = None }

	/// Filter out [`Playlist`] [`files`] from audio [`files`].
	///
	/// This function returns a [`Vec`] that contains all successfully parsed playlists.\
	/// The last item of the [`Vec`] is the so called outlier, items of the [`Iterator`] that could not be parsed to a playlist, and so are treated as tracks instead.
	///
	/// [`files`]: std::fs::File
	pub fn try_from_paths(iterator: impl IntoIterator<Item = String>) -> Result<Vec<Self>, Error> {
		let mut rest = Vec::with_capacity(8);
		let mut outliers = Playlist {
			song: Vec::with_capacity(8),
			time: None,
		};
		for path in iterator {
			match read_to_string(fmt_path(&path)?) { // might not always work (might sometimes be mp3 but still contain fully valid utf-8 'till the end)
				Ok(contents) => rest.push(Self::try_from_contents(contents)?),
				Err(_) => {
					outliers
						.song
						.push(
							Track {
								file: path.into_boxed_str(),
								time: None,
							}
						);
				},
			}
		}
		Ok(
			rest
				.into_iter()
				.filter(|list| !list.is_empty())
				.collect()
		)
	}

	/// Merge a list of [`Playlists`] into a single [`Playlist`].
	///
	/// [`Playlists`]: Playlist
	pub fn flatten(lists: Vec<Self>) -> Result<Self, Error> {
		let repeats = lists
			.iter()
			.min_by_key(|Self { time, .. }| time.unwrap_or_default())
			.map_or(Err(VectorError::EmptyVector), Ok)?
			.time
			.unwrap_or_default();
		let tracks: Vec<Track> = lists
			.into_iter()
			.flat_map(|list| list.song)
			.collect();
		Ok(
			Self {
				song: tracks,
				time: Some(repeats),
			}
		)
	}

	// /// Load each [`Track`]'s duration and stream.
	// ///
	// /// The function's output will be put into the global variable [`FILES`].\
	// /// This function also clears [`FILES`] when it successfully loads at least one [`Track`].
	// pub(crate) fn load_song(&self) -> Result<(), Error> {
	// 	let Self { song, .. } = self;

	// 	let startup_clear = Once::new();

	// 	for Track { file, .. } in song.iter() {
	// 		let formatted = fmt_path(file)?;

	// 		match (File::open(&formatted), read_from_path(formatted)) {
	// 			(Ok(contents), Ok(info)) => {
	// 				startup_clear.call_once(|| map_files_mut(Vec::clear));
	// 				map_files_mut(|files|
	// 					files.push(
	// 						MetaData {
	// 							stream: BufReader::new(contents),
	// 							duration: info
	// 								.properties()
	// 								.duration(),
	// 						}
	// 					)
	// 				);
	// 				continue
	// 			},
	// 			(Err(why), _) => Err(why)?,
	// 			(_, Err(why)) => Err(why)?,
	// 		}
	// 	}
	// 	Ok(())
	// }

	// /// Play the entire list back.
	// ///
	// /// # The Output's Meaning:
	// /// - [`true`]: the program has been manually exited.
	// /// - [`false`]: progress to the next playlist.
	// pub(crate) fn play(&mut self, bundle: &IOHandle, lists_index: &mut usize, volume: &mut f32) -> Result<bool, Error> { // bool = should exit or not.
	// 	let Self { ref mut song, .. } = self;

	// 	let old_lists_index = *lists_index;
	// 	let songs_length = song.len();
	// 	let mut songs_index = 0;

	// 	let controls = bundle.controls_get();

	// 	while songs_index < songs_length {
	// 		let old_songs_index = songs_index; // (sort of) proxy to index (used because of rewind code)
	// 		// unless something is very wrong with the index (old), this will not error.
	// 		let song = unsafe {
	// 			self
	// 				.song
	// 				.get_unchecked_mut(old_songs_index)
	// 		};


	// 		let playback = bundle.stream_play(get_file(old_songs_index))?;
	// 		if let Some(controls) = controls {
	// 			match song.play(playback, &mut songs_index, controls, volume)? {
	// 				Instruction::Hold => { },
	// 				Instruction::Next => {
	// 					*lists_index += 1;
	// 					return Ok(false)
	// 				},
	// 				Instruction::Back => {
	// 					*lists_index -= (old_lists_index > 0) as usize;
	// 					return Ok(false)
	// 				},
	// 				Instruction::Exit => return Ok(true),
	// 			}
	// 		} else { song.play_headless(playback, &mut songs_index, volume)? };
	// 		get_file(old_songs_index).rewind()?;
	// 		clear()?;
	// 	}
	// 	self.repeat_or_increment(lists_index);
	// 	Ok(false)
	// }

	/// [`is_empty`] delegate
	///
	/// [`is_empty`]: Vec::is_empty
	pub(crate) fn is_empty(&self) -> bool {
		self
			.song
			.is_empty()
	}

	#[inline(always)]
	/// Load a [`Playlist`] from a [`Path`] represented as a [`String`].
	///
	/// The string is, before being loaded, passed into the [`fmt_path`] function.
	///
	/// [`Path`]: std::path::Path
	fn try_from_contents(contents: String) -> Result<Self, Error> { from_str(&contents).map_err(Error::from) }
}

impl Track {
	pub fn set_time(&mut self, value: isize) { self.time = Some(value) }
	pub fn unset_time(&mut self) { self.time = None }

	// /// Play without a head.
	// ///
	// /// This will severily impare one's ability to control the playback.
	// pub fn play_headless(&mut self, playback: Sink, songs_index: &mut usize, volume: &mut f32)  -> Result<(), Error> {
	// 	playback.set_volume(volume.clamp(0.0, 2.0));

	// 	let mut elapsed = Duration::ZERO;
	// 	let duration = get_file(*songs_index).get_duration();
	// 	self.repeat_or_increment(songs_index);

	// 	while elapsed <= duration {
	// 		print!("\r[{}][{volume:>5.2}]\0",
	// 			{
	// 				let seconds = elapsed.as_secs();
	// 				let minutes = seconds / 60;
	// 				format!("{:0>2}:{:0>2}:{:0>2}", minutes / 60, minutes % 60, seconds % 60)
	// 			}
	// 		);
	// 		stdout().flush()?;

	// 		sleep(TICK);
	// 		elapsed += TICK
	// 	}
	// 	Ok(())
	// }
	// /// Play the track back.
	// pub fn play(&mut self, playback: Sink, songs_index: &mut usize, controls: &Controls, volume: &mut f32) -> Result<Instruction, Error> {
	// 	playback.set_volume(volume.clamp(0.0, 2.0));

	// 	let duration = get_file(*songs_index).get_duration();
	// 	let mut elapsed = Duration::ZERO;

	// 	while elapsed <= duration {
	// 		let paused = playback.is_paused();

	// 		print!("\r[{}][{volume:>5.2}]\0",
	// 			{
	// 				let seconds = elapsed.as_secs();
	// 				let minutes = seconds / 60;
	// 				format!("{:0>2}:{:0>2}:{:0>2}", minutes / 60, minutes % 60, seconds % 60)
	// 			}
	// 		);
	// 		stdout().flush()?;

	// 		let now = Instant::now();
	// 		elapsed += match controls.signal_receive(now) {
	// 			Err(RecvTimeoutError::Timeout) => if paused { continue } else { TICK },

	// 			Ok(_) => now.elapsed(),
	// 			// Ok(Layer::Playlist(signal)) => return signal.manage(elapsed),
	// 			// Ok(Layer::Track(signal)) => if signal.manage(&playback, elapsed, songs_index) { return Instruction::Hold } else { now.elapsed() },
	// 			// Ok(Layer::Volume(signal)) => signal.manage(&playback, now, volume),

	// 			Err(RecvTimeoutError::Disconnected) => {
	// 				// log!(; "receiving control-thread" DISCONNECTED);
	// 				return Ok(Instruction::Exit)
	// 			}, // chain reaction will follow
	// 		}
	// 	}
	// 	self.repeat_or_increment(songs_index);
	// 	Ok(Instruction::Hold)
	// }
}

// impl MetaData {
// 	/// Copy the underlying [`Duration`] of the held [`stream`]
// 	///
// 	/// [`stream`]: Self#field.stream
// 	pub(crate) fn get_duration(&self) -> Duration { self.duration }
// }

// impl Deref for MetaData {
// 	type Target = BufReader<File>;

// 	/// Simply returns a reference to the [`stream`].
// 	///
// 	/// [`stream`]: Self#field.stream
// 	fn deref(&self) -> &Self::Target { &self.stream }
// }

// impl DerefMut for MetaData {
// 	/// Returns a mutable reference to the [`stream`].
// 	///
// 	/// [`stream`]: Self#field.stream
// 	fn deref_mut(&mut self) -> &mut Self::Target { &mut self.stream }
// }
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
