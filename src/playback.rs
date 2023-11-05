///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use lofty::{
	AudioFile,
	read_from_path,
};
use std::{
	fs::File,
	cell::Cell,
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
use crossbeam_channel::RecvTimeoutError;
use super::{
	TICK,
	Error,
	VectorError,
	serde::{ SerDeTrack, SerDePlaylist },
	in_out::{ IOHandle, Signal },
	utilities::{ clear, fmt_path },
};
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
const STEP: f32 = 0.05;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// 
pub struct Playlist {
	/// Map of indexes that map directly to the vector of [`streams`]
	///
	/// [`streams`]: Self#field.streams
	track_map: Cell<Vec<usize>>,

	/// Maximum pointer offset.
	///
	/// Equates to [`Vec.len()`].
	///
	/// [`Vec.len()`]: Vec::len
	length: usize,

	///
	tracks: Vec<Track>,
	repeats: Cell<isize>,
}

/// A byte stream.
pub struct Track {
	stream: File,
	repeats: Cell<isize>,
	duration: Duration,
}

/// The player's state.
///
/// # Pointers
///
/// This structure holds two pointers that operate similar to coordinates on a grid.\
/// The first, and more important, pointer is the 'playlist-pointer.' It is responsible for, as the name sais, 
pub struct Playhandle {
	current_track_index: Cell<usize>,
	current_playlist_index: Cell<usize>,

	streams_vector: Vec<Playlist>,

	/// Global volume.
	volume: Cell<f32>,
	paused: Cell<bool>,
	//  1.0 + 2.0 * -1.0 = -1.0
	// -1.0 + 2.0 *  1.0 =  1.0

	io_handle: IOHandle,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(debug_assertions, derive(Debug))]
#[derive(Default)]
/// I have no clue why.
///
/// # Skip-Levels
///
/// A skip can have a level that denominates how many function layers it can pass through.\
/// For example: A level 2 skip can go through function A and B, whilst a level 1 skip can only go up to A.
pub enum ControlFlow {
	Break,
	/// A [level] 1 skip.
	///
	/// [level]: Self#Skip-levels
	Skip,
	/// A [level] 2 skip.
	///
	/// [level]: Self#Skip-levels
	SkipSkip,
	#[default] Default,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl Playlist {
	#[inline(always)]
	pub fn tracks_count(&self) -> usize {
		self
			.tracks
			.len()
	}

	#[inline(always)]
	pub fn tracks_is_empty(&self) -> bool {
		self
			.tracks
			.is_empty()
	}

	pub fn play_with(&self, handle: &Playhandle) -> Result<ControlFlow, Error> {
		while handle
			.track_index_check()
			.is_none()
		{
			match unsafe {
				self
					.nth_unchecked(handle.track_index_get_unchecked())
					.play_with(handle)
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
			return self.play_with(handle)
		}
		let _ = handle.playlist_index_try_set(|old| old + 1);
		Ok(().into())
	}

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

	/// Mutable counterpart to [`nth`]
	///
	/// # Safety
	///
	/// - This function will return [`None`] if the provided index is out of bounds.
	///
	/// [`nth`]: Self.nth
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
					if track_map.is_empty() { Err(VectorError::EmptyVector)? }
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
	pub fn play_with(&self, data: &Playhandle) -> Result<ControlFlow, Error> {
		let mut stream = Vec::with_capacity(127);
		self
			.stream
			.try_clone()?
			.read_to_end(&mut stream)?;
		let pointer =  stream.as_slice() as *const [u8];

		data.stream_play(Cursor::new(unsafe { &*pointer }))?;
		let controls = data
			.io_handle_get()
			.controls_get();

		let mut whole_elapsed_time = Duration::ZERO;

		let decrement: fn(usize) -> usize = |old| old - (old > 0) as usize;
		let increment: fn(usize) -> usize = |old| old + 1;

		data.playback_play();
		while whole_elapsed_time < self.duration {
			let moment_capture = Instant::now();

			data.player_display(whole_elapsed_time)?;
			match controls.signal_receive(moment_capture) {
				Err(RecvTimeoutError::Timeout) => {
					if !data.playback_is_paused() { whole_elapsed_time += TICK }
					continue
				},
				Ok(Signal::PlaylistNext) => {
					data.playback_clear();
					clear()?;
					data.playlist_index_try_set(increment)?;
					return Ok(ControlFlow::SkipSkip)
				},
				Ok(Signal::PlaylistBack) => {
					data.playback_clear();
					clear()?;
					data.playlist_index_try_set(decrement)?;
					return Ok(ControlFlow::SkipSkip)
				},
				Ok(Signal::Exit) => return Ok(ControlFlow::Break),

				Ok(Signal::TrackNext) => {
					data.playback_clear();
					clear()?;
					data.track_index_try_set(increment)?;
					return Ok(ControlFlow::Skip)
				},
				Ok(Signal::TrackBack) => {
					data.playback_clear();
					clear()?;
					data.track_index_try_set(decrement)?;
					return Ok(ControlFlow::Skip)
				},
				Ok(Signal::Play) => data.playback_toggle(),

				Ok(Signal::VolumeIncrease) => data.volume_increment(),
				Ok(Signal::VolumeDecrease) => data.volume_decrement(),
				Ok(Signal::Mute) => data.volume_mute(),

				Err(RecvTimeoutError::Disconnected) => Err(Error::ChannelDisconnect)?,
			}

			data.volume_update();
			if let Some(error) = data.playlist_index_check() { Err(error)? };
			if let Some(error) = data.track_index_check() { Err(error)? };


			if !data.playback_is_paused() { whole_elapsed_time += moment_capture.elapsed() }
		}
		if self.repeats_can() {
			self.repeats_update();
			return self.play_with(data)
		}
		data.track_index_try_set(increment)?;
		Ok(().into())
	}

	#[inline]
	pub fn repeats_can(&self) -> bool {
		self
			.repeats
			.get() != 0
	}

	#[inline]
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
		let path = fmt_path(file)?;

		let duration = read_from_path(&path)?
			.properties()
			.duration();
		Ok(
			Self {
				stream: File::open(path)?,
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
	/// This functions is equivalent to a [`Vec.len()`] call.
	///
	/// [`Playlists`]: Playlist
	/// [`Vec.len()`]: Vec::len
	pub fn entries_count(&self) -> usize {
		self
			.streams_vector
			.len()
	}

	#[inline(always)]
	/// Find out if a there are any held [`Playlists`].
	///
	/// This function is equal to a [`Vec.is_empty()`] call.
	///
	/// [`Playlists`]: Playlist
	/// [`Vec.is_empty()`]: Vec::is_empty
	pub fn entries_is_empty(&self) -> bool {
		self
			.streams_vector
			.is_empty()
	}

	/// Display the default player.
	///
	/// The default player is: `[hh:mm:ss][vol.]`
	pub fn player_display(&self, elapsed: Duration) -> Result<(), Error> {
		print!("\r[{}][{:>5.2}]\0",
			{
				let seconds = elapsed.as_secs();
				let minutes = seconds / 60;
				format_args!("{:0>2}:{:0>2}:{:0>2}", minutes / 60, minutes % 60, seconds % 60)
			},
			self
				.volume
				.get(),
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
	pub fn all_streams_play(&mut self) -> Result<ControlFlow, Error>  {
		while self
			.playlist_index_check()
			.is_none()
		{
			let index = unsafe { self.playlist_index_get_unchecked() };
			unsafe {
				self
					.streams_vector
					.get_unchecked_mut(index)
					.shuffle()
			}
			match unsafe {
				self
					.streams_vector
					.get_unchecked(index)
					.play_with(self)?
			} {
				ControlFlow::Break => return Ok(ControlFlow::Break),
				ControlFlow::Skip => { }, // NOTE(by: @OST-Gh): assume index math already handled.
				ControlFlow::SkipSkip => unimplemented!(), // NOTE(by: @OST-Gh): cannot return level-2 skip at playlist level.
				ControlFlow::Default => {
					clear()?;
					if self.entries_count() - 1 <= unsafe { self.playlist_index_get_unchecked() } { return Ok(().into()) }
				},
			}
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
		let index = self
			.current_playlist_index
			.get();
		let maximum = self
			.streams_vector
			.len();
		(index >= maximum).then_some(VectorError::OutOfBounds)
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
		let index = self
			.current_track_index
			.get();
		let maximum = unsafe {
			self
				.streams_vector
				.get_unchecked(playlist_index)
				.tracks_count()
		};
		(index >= maximum).then_some(VectorError::OutOfBounds)
	}

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
		self
			.current_playlist_index
			.set(new_index);
		if let Some(error) = self.playlist_index_check() {
			self
				.current_playlist_index
				.set(old_index);
			Err(error)?
		}
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
		self
			.current_track_index
			.set(new_index);
		if let Some(error) = self.track_index_check() {
			self
				.current_track_index
				.set(old_index);
			Err(error)?
		};
		Ok(())
	}

	pub fn io_handle_get(&self) -> &IOHandle { &self.io_handle }
	pub fn io_handle_take(self) -> IOHandle { self.io_handle }

	#[inline]
	/// Get a clamped version of the internal volume.
	///
	/// This function is clamping the internal [`f32`], volume between 0 and 2.
	pub fn volume_get(&self) -> f32 {
		self
			.volume
			.get()
			.clamp(0.0, 2.0)
	}

	#[inline]
	/// Set the volume based on the clamped output of [`volume_get`].
	///
	/// [`volume_get`]: Self::volume_get
	pub fn volume_set(&self, map: impl FnOnce(f32) -> f32) {
		self
			.volume
			.set(map(self.volume_get()).clamp(-1.0, 2.0))
	}

	#[inline]
	/// Set the volume based on the raw internal [`f32`].
	pub fn volume_set_raw(&self, map: impl FnOnce(f32) -> f32) {
		self
			.volume
			.set(
				map(
					self
						.volume
						.get()
				)
					.clamp(-1.0, 2.0)
			)
	}

	#[inline]
	/// A low level mute function.
	///
	/// Call [`volume_update`] to take effect.
	///
	/// [`volume_update`]: Self::volume_update
	pub fn volume_mute(&self) { self.volume_set_raw(|old| old + 2.0 * -old) }

	#[inline]
	/// A low level dial up function.
	///
	/// Counterpart: [`volume_decrement`].
	///
	/// Call [`volume_update`] to take effect.
	///
	/// [`volume_decrement`]: Self::volume_decrement
	/// [`volume_update`]: Self::volume_update
	pub fn volume_increment(&self) { self.volume_set_raw(|old| old + STEP) }

	#[inline]
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

	#[inline]
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

	pub fn raw_parts_from(io_handle: IOHandle, streams_vector: Vec<Playlist>) -> Self {
		Self {
			current_track_index: Cell::new(0),
			current_playlist_index: Cell::new(0),

			streams_vector,
		
			volume: Cell::new(1.0),
			paused: Cell::new(
				io_handle
					.playback_get()
					.is_paused()
			),

			io_handle,
		}
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
