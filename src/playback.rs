///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
//! Playback essential structures are found here.
//!
//! This module's structures should be able to manipulate themselves, even if they are not declared mutable.\
//! In order to achieve that, the structures encapuslate the mutatable parts in [`Cells`].
//!
//! [`Cells`]: std::cell::Cell
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use lofty::{
	AudioFile,
	read_from_path,
};
use std::{
	fs::File,
	path::PathBuf,
	cell::Cell,
	thread::sleep,
	time::{ Duration, Instant },
	io::{
		Cursor,
		Read,
		Seek,
		Write,
		stdout,
	},
};
use fastrand::Rng;
use crossbeam_channel::TryRecvError;
use super::{
	Error,
	VectorError,
	ChannelError,
	serde::{ SerDeTrack, SerDePlaylist },
	in_out::{ IOHandle, Signal },
	utilities::{ clear, fmt_path },
};
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
const STEP: f32 = 0.025;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// A collection of [`Tracks`].
///
/// This structure maintains two [`Vecs`]:
/// - one pointer-map that is used to map 
///
/// [`Tracks`]: Track
/// [`Vecs`]: Vec
pub struct Playlist {
	/// Map of indexes that map directly to the vector of [`streams`]
	///
	/// [`streams`]: Self#field.streams
	track_map: Cell<Vec<usize>>,

	/// Maximum pointer offset.
	///
	/// Equates to [`len`].
	///
	/// [`len`]: Vec::len
	length: usize,
	tracks: Vec<Track>,
	repeats: Cell<isize>,
}

/// A byte stream.
pub struct Track {
	file_path: PathBuf,
	repeats: Cell<isize>,
	duration: Duration,
}

/// The player's state.
///
/// This is a singleton structure, of which (preferably) only one is active at a time.
///
/// # Pointers
///
/// This structure holds two pointers that operate similar to coordinates on a grid.\
/// The first, and more important, pointer is the 'playlist-pointer.' It is responsible for, as the name sais, 
pub struct Playhandle {
	current_track_index: Cell<usize>,
	current_playlist_index: Cell<usize>,

	has_reached_current_playlist_end: Cell<bool>,
	has_reached_entire_end: Cell<bool>,

	playlists: Vec<Playlist>,

	/// Global volume.
	volume: Cell<f32>,
	paused: Cell<bool>,
	//  1.0 + 2.0 * -1.0 = -1.0
	// -1.0 + 2.0 *  1.0 =  1.0

	io_handle: IOHandle,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(any(debug_assertions, feature = "debug"), derive(Debug))]
#[derive(Default)]
/// Singals returned by some crucial functions.
///
/// These signals are here to indicate exit states.\
/// # Skip-Levels
///
/// A skip can have a level that denominates how many function layers it can pass through.\
/// For example: A level 2 skip can go through function A and B, whilst a level 1 skip can only go up to A.
pub enum ControlFlow {
	/// Don't continue if even possible.
	Break,
	/// A [level] 1 skip.
	///
	/// [level]: Self#Skip-levels
	Skip,
	/// A [level] 2 skip.
	///
	/// [level]: Self#Skip-levels
	SkipSkip,
	/// The function finished without any special exceptions.
	#[default] Default,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl Playlist {
	#[inline(always)]
	/// Get the amount of held [`Tracks`].
	///
	/// [`Tracks`]: Track
	pub fn tracks_count(&self) -> usize {
		self
			.tracks
			.len()
	}

	#[inline(always)]
	/// A specialisation of [`tracks_count`].
	///
	/// This function compares the amount of held [`Tracks`] to zero.
	///
	/// [`tracks_count`]: Self::tracks_count
	/// [`Tracks`]: Track
	pub fn tracks_is_empty(&self) -> bool { self.tracks_count() == 0 }

	pub fn play_through(&self, handle: &Playhandle) -> Result<ControlFlow, Error> {
		while handle
			.track_index_check()
			.is_none()
		{
			match unsafe {
				self
					.nth_unchecked(handle.track_index_get_unchecked())
					.play_through(handle)
			} {
				Ok(ControlFlow::Break) => return Ok(ControlFlow::Break),
				Ok(ControlFlow::SkipSkip) => return Ok(ControlFlow::Skip),
				Ok(ControlFlow::Skip) => continue,
				Ok(ControlFlow::Default) => { },
				Err(Error::Vector(VectorError::OutOfBounds)) => { // NOTE(by: @OST-Gh): assume track-ptr's poisoned.
					let _ = handle.track_index_try_set(|_| 0);
					break
				},
				Err(other) => Err(other)?,
			}
		}
		if self.repeats_can() {
			self.repeats_update();
			self.shuffle();
			let _ = handle.track_index_try_set(|_| 0); // NOTE(by: @OST-Gh): should not error.
			return self.play_through(handle)
		}
		let _ = handle.playlist_index_try_set(|old| old + 1);
		Ok(().into())
	}

	/// Shuffle all [`Tracks`] around.
	///
	/// The Shuffling works with the help of a [random number generator].
	///
	/// [`Tracks`]: Track
	/// [random number generator]: Rng
	pub fn shuffle(&self) {
		let mut map = self
			.track_map
			.take();
		let mut generator = Rng::new();
		generator.shuffle(&mut map);


		for index in 0..self.length {
			map.swap(index, generator.usize(0..=index));
			map.swap(index, generator.usize(index..self.length));
			// a b c; b inclusive in both random ranges
			// b a c
			// b c a
		}

		self
			.track_map
			.set(map)
	}

	#[inline(always)]
	/// Get the correctly mapped index.
	///
	/// # Safety
	///
	/// - This function will return [`None`] if the provided index is out of bounds.
	pub fn index_get(&self, index: usize) -> Option<usize> {
		let map = self
			.track_map
			.take();
		let mapped_index = map
			.get(index)
			.map(usize::clone);
		self
			.track_map
			.set(map);
		mapped_index
	}

	#[inline(always)]
	/// Get the correctly mapped index without bound checking.
	///
	/// # Safety
	///
	/// - It is undefined behaviour to index outside of a [`slice`]'s bounds.
	pub unsafe fn index_get_unchecked(&self, index: usize) -> usize {
		let map = self
			.track_map
			.take();
		let mapped_index = unsafe { *map.get_unchecked(index) };
		self
			.track_map
			.set(map);
		mapped_index
	}

	#[inline]
	/// Get the nth mapped index's [`Track`].
	///
	/// # Safety
	///
	/// - This function will return [`None`] if the provided index is out of bounds.
	pub fn nth(&self, index: usize) -> Option<&Track> {
		self
			.index_get(index)
			.map(|index|
				unsafe {
					self
						.tracks
						.get_unchecked(index)
				}
			)
	}

	#[inline]
	/// Mutable counterpart to [`nth`]
	///
	/// # Safety
	///
	/// - This function will return [`None`] if the provided index is out of bounds.
	///
	/// [`nth`]: Self::nth
	pub fn nth_mut(&mut self, index: usize) -> Option<&mut Track> {
		self
			.index_get(index)
			.map(|index|
				unsafe {
					self
						.tracks
						.get_unchecked_mut(index)
				}
			)
	}

	#[inline]
	/// Get the nth mapped index's [`Track`] without bound checking.
	///
	/// # Safety
	///
	/// - It is undefined behaviour to index outside of a [`slice`]'s bounds.
	pub unsafe fn nth_unchecked(&self, index: usize) -> &Track {
		self
			.tracks
			.get_unchecked(self.index_get_unchecked(index))
	}

	#[inline]
	/// Mutable counterpart to [`nth_unchecked`]
	///
	/// # Safety
	///
	/// - It is undefined behaviour to index outside of a [`slice`]'s bounds.
	///
	/// [`nth_unchecked`]: Self::nth_unchecked
	pub unsafe fn nth_unchecked_mut(&mut self, index: usize) -> &mut Track {
		let mapped_index = self.index_get_unchecked(index);
		self
			.tracks
			.get_unchecked_mut(mapped_index)
	}

	#[inline]
	/// See if the [`Playlist`] can repeat.
	pub fn repeats_can(&self) -> bool {
		self
			.repeats
			.get() != 0
	}

	#[inline]
	/// Decrement the number of repeats.
	pub fn repeats_update(&self) {
		let old = self
			.repeats
			.get();
		self
			.repeats
			.set(old - 1);
	}
}

impl TryFrom<SerDePlaylist> for Playlist {
	type Error = Error;

	fn try_from(SerDePlaylist { song, time }: SerDePlaylist) -> Result<Self, Error> {
		match song
			.into_iter()
			.enumerate()
			.map(|(index, track)| Ok((index, track.try_into()?)))
			.collect::<Result<Vec<(usize, Track)>, Error>>()
			.map(|tuple|
				{
					let (track_map, tracks): (Vec<usize>, Vec<Track>) = tuple
						.into_iter()
						.unzip();
					if track_map.is_empty() { Err(VectorError::Empty)? }
					Ok(
						Self {
							track_map: Cell::new(track_map),
							length: tracks.len(),
							tracks,
							repeats: Cell::new(time.unwrap_or_default()),
						}
					)
				}
			)
		{
			Err(error) | Ok(Err(error)) => Err(error)?,
			Ok(Ok(playlist)) => Ok(playlist),
		}
	}
}

impl Track {
	/// Load the file, and play it back.
	pub fn play_through(&self, data: &Playhandle) -> Result<ControlFlow, Error> {
		let mut stream = Vec::with_capacity(127);
		File::open(&self.file_path)?.read_to_end(&mut stream)?;
		data.stream_play(Cursor::new(unsafe { &*(stream.as_slice() as *const [u8]) }))?; // NOTE(by: @OST-Gh): yep, a fucking lifetime hoist via raw pointers.

		let controls = data
			.io_handle_get()
			.controls_get();
		let mut whole_elapsed_time = Duration::ZERO;
		let decrement: fn(usize) -> usize = |old| old - (old > 0) as usize;
		let increment: fn(usize) -> usize = |old| old + 1;

		data.playback_play();
		while whole_elapsed_time < self.duration {
			let moment = Instant::now();
			data.player_display(whole_elapsed_time)?;

			sleep(Duration::from_nanos(1)); // NOTE(by: @OST-Gh): for some reason: without this, the player's time won't update.
			match controls.signal_receive() {
				Err(TryRecvError::Empty) => { },
				Ok(Signal::Exit) => {
					data.playback_clear();
					clear()?;
					return Ok(ControlFlow::Break)
				},

				Ok(signal) if signal.is_skip() => {
					data.playback_clear();
					clear()?;
					let setter = signal
						.is_next_skip()
						.then_some(increment)
						.unwrap_or(decrement);
					signal
						.is_track_skip()
						.then_some::<fn(&Playhandle, fn(usize) -> usize) -> Result<(), VectorError>>(|data: &Playhandle, setter| data.track_index_try_set(setter))
						.unwrap_or(|data: &Playhandle, setter| data.playlist_index_try_set(setter))(&data, setter)?;
					return Ok(ControlFlow::Skip)
				},
				Ok(Signal::Play) => data.playback_toggle(),

				Ok(signal) if signal.is_volume() => {
					match signal {
						Signal::VolumeIncrease => data.volume_increment(),
						Signal::VolumeDecrease => data.volume_decrement(),
						Signal::Mute => data.volume_mute(),
						_ => unreachable!(),
					}
					data.volume_update()
				},

				Ok(_) => unreachable!(),

				Err(TryRecvError::Disconnected) => Err(ChannelError::Disconnect)?,
			}

			if !data.playback_is_paused() { whole_elapsed_time += moment.elapsed() }
		}
		if self.repeats_can() {
			self.repeats_update();
			return self.play_through(data)
		}
		data.track_index_try_set(increment)?;
		Ok(().into())
	}

	#[inline(always)]
	/// wether or not a [`Track`] can repeat.
	pub fn repeats_can(&self) -> bool {
		self
			.repeats
			.get() != 0
	}

	#[inline]
	/// Decrement the repeat count.
	pub fn repeats_update(&self) {
		let old = self
			.repeats
			.get();
		self
			.repeats
			.set(old - 1);
	}
}

impl TryFrom<SerDeTrack> for Track {
	type Error = Error;

	fn try_from(SerDeTrack { file, time }: SerDeTrack) -> Result<Self, Error> {
		let file_path = fmt_path(file)?;

		let duration = read_from_path(&file_path)?
			.properties()
			.duration();
		Ok(
			Self {
				file_path,
				repeats: Cell::new(time.unwrap_or_default()),
				duration,
			}
		)
	}
}

impl Playhandle {
	#[inline(always)]
	/// Count the number of held [`Playlists`].
	///
	/// This functions is equivalent to a [`len`] call.
	///
	/// [`Playlists`]: Playlist
	/// [`len`]: Vec::len
	pub fn entries_count(&self) -> usize {
		self
			.playlists
			.len()
	}

	#[inline(always)]
	/// A specialisation of [`entries_count`].
	///
	/// This function compares the amount of held [`Playlists`] to zero.
	///
	/// [`entries_count`]: Self::entries_count
	/// [`Playlists`]: Playlist
	pub fn entries_is_empty(&self) -> bool {
		self
			.playlists
			.is_empty()
	}

	#[inline(always)]
	/// See if all [`Tracks`] of a [`Playlist`] have been played through.
	///
	/// This function is single-use.
	///
	/// [`Tracks`]: Track
	pub fn playlist_has_ended(&self) -> bool {
		self
			.has_reached_current_playlist_end
			.take()
	}

	#[inline(always)]
	/// See if all [`Playlists`] have been played through.
	///
	/// This function is single-use.
	///
	/// [`Playlists`]: Playlist
	pub fn all_playlists_have_ended(&self) -> bool {
		self
			.has_reached_entire_end
			.take()
	}

	/// Display the default player.
	///
	/// The default player is: `[hh:mm:ss][vol.]`
	pub fn player_display(&self, elapsed: Duration) -> Result<(), Error> {
		print!("\r[{}][{:>6.3}]\0",
			{
				let seconds = elapsed.as_secs();
				let minutes = seconds / 60;
				format_args!("{:0>2}:{:0>2}:{:0>2}", minutes / 60, minutes % 60, seconds % 60)
			},
			self.volume_get_raw(),
		);
		stdout()
			.flush()
			.map_err(Error::Io)
	}

	/// Play all [`Playlists`] back.
	///
	/// See [`ControlFlow`] for more information on the returned data's meanings.
	///
	/// [`Playlists`]: Playlist
	pub fn all_playlists_play(&mut self, should_shuffle: bool) -> Result<ControlFlow, Error>  {
		while self
			.playlist_index_check()
			.is_none()
		{
			let index = unsafe { self.playlist_index_get_unchecked() };
			if should_shuffle {
				unsafe {
					self
						.playlists
						.get_unchecked_mut(index)
						.shuffle()
				}
			}
			match unsafe {
				self
					.playlists
					.get_unchecked(index)
					.play_through(self)?
			} {
				ControlFlow::Break => return Ok(ControlFlow::Break),
				ControlFlow::Skip => { }, // NOTE(by: @OST-Gh): assume index math already handled.
				ControlFlow::SkipSkip => unimplemented!(), // NOTE(by: @OST-Gh): cannot return level-2 skip at playlist level.
				ControlFlow::Default => clear()?,
			}
			if self.playlist_has_ended() || self.all_playlists_have_ended() { return Ok(().into()) }
		}
		Ok(().into())
	}

	#[inline(always)]
	/// Play a single source back.
	pub fn stream_play(&self, source: impl Read + Seek + Send + Sync + 'static) -> Result<(), Error> {
		self
			.io_handle
			.stream_play(source)
	}

	#[inline]
	/// Make sure that the playlist-pointer is not [out of bounds]
	///
	/// # Returns:
	///
	/// Returns [`None`] if there is no errors.
	///
	/// [out of bounds]: VectorError::OutOfBounds
	pub fn playlist_index_check(&self) -> Option<VectorError> {
		(
			self
				.current_playlist_index
				.get() >= self.entries_count()
		)
			.then_some(VectorError::OutOfBounds)
	}

	#[inline]
	/// Make sure that the track-pointer is not [out of bounds]
	///
	/// # Returns:
	///
	/// Returns [`None`] if there is no errors.
	///
	/// [out of bounds]: VectorError::OutOfBounds
	pub fn track_index_check(&self) -> Option<VectorError> {
		let playlist_index = match self.playlist_index_get() {
			Ok(index) => index,
			Err(error) => return Some(error),
		};
		let maximum = unsafe {
			self
				.playlists
				.get_unchecked(playlist_index)
				.tracks_count()
		};
		(
			self
				.current_track_index
				.get() >= maximum
		)
			.then_some(VectorError::OutOfBounds)
	}

	#[inline]
	/// Get the playlist-pointer.
	pub fn playlist_index_get(&self) -> Result<usize, VectorError> {
		self
			.playlist_index_check()
			.map_or_else(
				||
				Ok(unsafe { self.playlist_index_get_unchecked() }),
				Err,
			)
	}

	#[inline]
	/// Get the track-pointer.
	pub fn track_index_get(&self) -> Result<usize, VectorError> {
		self
			.playlist_index_check()
			.map_or_else(
				||
				Ok(unsafe { self.track_index_get_unchecked() }),
				Err,
			)
	}

	#[inline(always)]
	/// Get the playlist-pointer without checking if it has overrun the maximum.
	///
	/// # Safety
	///
	/// - This function corresponds to a basically returning the raw held pointer.
	pub unsafe fn playlist_index_get_unchecked(&self) -> usize {
		self
			.current_playlist_index
			.get()
	}
	#[inline(always)]
	/// Get the track-pointer without checking if it has overrun the maximum.
	///
	/// # Safety
	///
	/// - This function corresponds to a basic return of the raw held pointer.
	pub unsafe fn track_index_get_unchecked(&self) -> usize {
		self
			.current_track_index
			.get()
	}

	/// Attempt to set the playlist-pointer to the output of the input closure.
	///
	/// # Safety
	///
	/// - This function will reset back to the original value of the pointer if the output fails the checks of [`playlist_index_check`].
	///
	/// [`playlist_index_check`]: Self::playlist_index_check
	pub fn playlist_index_try_set(&self, setter: impl FnOnce(usize) -> usize) -> Result<(), VectorError> {
		let _ = self.track_index_try_set(|_| 0);
		let old_index = unsafe { self.playlist_index_get_unchecked() };
		let new_index = setter(old_index);
		if new_index >= self.entries_count() {
			self
				.has_reached_entire_end
				.set(true);
			Err(VectorError::OutOfBounds)?
		}
		self
			.current_playlist_index
			.set(new_index);
		Ok(())
	}

	/// Attempt to set the track-pointer to the output of the input closure.
	///
	/// # Safety
	///
	/// - This function will reset back to the original value of the pointer if the output fails the checks of [`track_index_check`].
	///
	/// [`track_index_check`]: Self::track_index_check
	pub fn track_index_try_set(&self, setter: impl FnOnce(usize) -> usize) -> Result<(), VectorError> {
		let old_index = unsafe { self.track_index_get_unchecked() };
		let new_index = setter(old_index);
		let playlist_index = match self.playlist_index_get() {
			Ok(index) => index,
			Err(error) => Err(error)?,
		};
		let maximum = unsafe {
			self
				.playlists
				.get_unchecked(playlist_index)
				.tracks_count()
		};
		if new_index >= maximum {
			self
				.has_reached_current_playlist_end
				.set(true);
			Err(VectorError::OutOfBounds)?
		}
		self
			.current_track_index
			.set(new_index);
		Ok(())
	}

	#[inline(always)]
	pub fn io_handle_get(&self) -> &IOHandle { &self.io_handle }
	#[inline(always)]
	pub fn io_handle_take(self) -> IOHandle { self.io_handle }

	#[inline]
	/// Get a clamped version of the internal volume.
	///
	/// This function is clamping the internal [`f32`], volume between 0 and 2.
	pub fn volume_get(&self) -> f32 {
		self
			.volume_get_raw()
			.clamp(0.0, 2.0)
	}

	#[inline]
	pub fn volume_get_raw(&self) -> f32 {
		self
			.volume
			.get()
	}

	#[inline]
	/// Set the volume based on the clamped output of [`volume_get`].
	///
	/// [`volume_get`]: Self::volume_get
	pub fn volume_set(&self, map: impl FnOnce(f32) -> f32) {
		self
			.volume
			.set(map(self.volume_get()))
	}

	#[inline(always)]
	/// Set the volume based on the raw internal [`f32`].
	pub fn volume_set_raw(&self, map: impl FnOnce(f32) -> f32) {
		self
			.volume
			.set(map(self.volume_get_raw()))
	}

	#[inline(always)]
	/// A low level mute function.
	///
	/// Call [`volume_update`] to take effect.
	///
	/// [`volume_update`]: Self::volume_update
	pub fn volume_mute(&self) { self.volume_set_raw(|old| old + 2.0 * -old) }

	#[inline(always)]
	/// A low level dial up function.
	///
	/// Counterpart: [`volume_decrement`].
	///
	/// Call [`volume_update`] to take effect.
	///
	/// [`volume_decrement`]: Self::volume_decrement
	/// [`volume_update`]: Self::volume_update
	pub fn volume_increment(&self) { self.volume_set_raw(|old| old + STEP) }

	#[inline(always)]
	/// A low level dial down function.
	///
	/// Counterpart: [`volume_increment`].
	///
	/// Call [`volume_update`] to take effect.
	///
	/// [`volume_increment`]: Self::volume_increment
	/// [`volume_update`]: Self::volume_update
	pub fn volume_decrement(&self) { self.volume_set_raw(|old| old - STEP) }

	/// Update the volume on the internal [`Sink`].
	///
	/// [`Sink`]: rodio::Sink
	pub fn volume_update(&self) {
		self
			.io_handle
			.playback_get()
			.set_volume(self.volume_get());
	}

	#[inline(always)]
	/// A low level play function.
	///
	/// Counterpart: [`playback_pause`].\
	/// High level: [`playback_toggle`].
	///
	/// [`playback_pause`]: Self::playback_pause
	/// [`playback_toggle`]: Self::playback_toggle
	pub fn playback_play(&self) {
		self
			.io_handle
			.playback_get()
			.play();
		self
			.paused
			.set(false)
	}

	#[inline(always)]
	/// A low level pause function.
	///
	/// Counterpart: [`playback_play`].\
	/// High level: [`playback_toggle`].
	///
	/// [`playback_play`]: Self::playback_play
	/// [`playback_toggle`]: Self::playback_toggle
	pub fn playback_pause(&self) {
		self
			.io_handle
			.playback_get()
			.pause();
		self
			.paused
			.set(true)
	}

	#[inline(always)]
	/// A high level combination of [`playback_play`] and [`playback_pause`].
	///
	/// [`playback_play`]: Self::playback_play
	/// [`playback_pause`]: Self::playback_pause
	pub fn playback_toggle(&self) {
		if self.playback_is_paused() { self.playback_play() } else { self.playback_pause() }
	}

	#[inline]
	/// A low level clear function.
	///
	/// This function clears and pauses the internal [`Sink`]
	///
	/// [`Sink`]: rodio::Sink
	pub fn playback_clear(&self) {
		self
			.io_handle
			.playback_get()
			.clear()
	}

	#[inline(always)]
	/// Find out if the internal [`Sink`] is paused or not.
	///
	/// [`Sink`]: rodio::Sink
	pub fn playback_is_paused(&self) -> bool {
		self
			.paused
			.get()
	}

	/// Initialise a new instance from the input.
	pub fn raw_parts_from(io_handle: IOHandle, streams_vector: Vec<Playlist>) -> Self {
		Self {
			current_track_index: Cell::new(0),
			current_playlist_index: Cell::new(0),

			has_reached_current_playlist_end: Cell::new(false),
			has_reached_entire_end: Cell::new(false),

			playlists: streams_vector,
		
			volume: Cell::new(1.0),
			paused: Cell::new(
				io_handle
					.playback_get()
					.is_paused()
			),

			io_handle,
		}
	}

	pub fn playlists_swap(&mut self, new: Vec<Playlist>) {
		self.playback_clear();
		self.playlists = new;
		let _ = self.track_index_try_set(|_| 0); // NOTE(by: @OST-Gh): Pray this doesn't error
		let _ = self.playlist_index_try_set(|_| 0);
	}
}

impl TryFrom<Vec<Playlist>> for Playhandle {
	type Error = Error;

	#[inline(always)]
	/// Try to instantiate a new [`IOHandle`], instead of passing it into the function.
	fn try_from(streams_vector: Vec<Playlist>) -> Result<Self, Error> {
		IOHandle::try_new().map(|io_handle| Self::raw_parts_from(io_handle, streams_vector))
	}
}

impl From<()> for ControlFlow {
	/// Convinience implementation.
	///
	/// [`Unit`] equates to [`Default`]
	///
	/// [`Unit`]: unit
	/// [`Default`]: Self::Default
	fn from(_: ()) -> Self { Self::Default }
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
