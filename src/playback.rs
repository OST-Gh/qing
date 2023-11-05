///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use lofty::{
	AudioFile,
	read_from_path,
};
use std::{
	fs::File,
	time::{ Duration, Instant },
	cell::Cell,
	thread::sleep,
	// mem::MaybeUninit,
	// ptr::NonNull,
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
	UnwrapError,
	serde::{ SerDeTrack, SerDePlaylist },
	in_out::{ IOHandle, Signal },
	utilities::{ clear, fmt_path },
};
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
const STEP: f32 = 0.05;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
pub struct Playlist {
	/// Map of indexes that map directly to the vector of [`streams`]
	///
	/// [`streams`]: Self#field.streams
	stream_map: Cell<Vec<usize>>,
	streams: Vec<Track>,
	repeats: Cell<isize>,
}

pub struct Track {
	stream: Vec<u8>,
	repeats: Cell<isize>,
	duration: Duration,
}

/// The player's state.
pub struct Playhandle {
	current_track_index: Cell<usize>,
	current_playlist_index: Cell<usize>,

	streams_vector: Vec<Playlist>,

	/// Global volume.
	volume: Cell<f32>,
	paused: Cell<bool>,
	//  1.0 + 2.0 * -1.0 = -1.0
	// -1.0 + 2.0 *  1.0 =  1.0

	io: IOHandle,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(debug_assertions, derive(Debug))]
#[derive(Default)]
pub enum ControlFlow {
	Break,
	Skip,
	SkipSkip,
	#[default] Default,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl Playlist {
	#[inline(always)]
	pub fn entry_count(&self) -> usize {
		self
			.streams
			.len()
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
					let _ = handle.track_index_set(|_| 0);
					break
				},
				Err(other) => Err(other)?,
			}
		}
		if self.repeats_can() {
			self.repeats_update();
			self.shuffle();
			let _ = handle.track_index_set(|_| 0); // NOTE(by: @OST-Gh): should not error.
			return self.play_with(handle)
		}
		let _ = handle.playlist_index_set(|old| old + 1);
		Ok(().into())
	}

	pub fn play_headless_with(&self, handle: &Playhandle) -> Result<(), Error> {
		while handle
			.track_index_check()
			.is_none()
		{
			unsafe {
				self
					.nth_unchecked(handle.track_index_get_unchecked())
					.play_headless_with(handle)?
			}
		}
		Ok(())
	}

	pub fn shuffle(&self) {
		let mut map = self
			.stream_map
			.take();
		let length = map.len();

		let mut generator = Rng::new();
		generator.shuffle(&mut map);

		for value in 0..length {
			let index = value % length;
			map.swap(index, generator.usize(0..=index));
			map.swap(index, generator.usize(index..length));
			// a b c; b inclusive in both random ranges
			// b a c
			// b c a
		}
		self
			.stream_map
			.set(map)
	}

	#[inline(always)]
	/// Get the correctly mapped index.
	///
	/// # Safety
	///
	/// - This function will return [`None`] if the provided index is out of bounds.
	fn get_index(&self, index: usize) -> Option<usize> {
		let map = self
			.stream_map
			.take();
		let mapped_index = map
			.get(index)
			.map(usize::clone);
		self
			.stream_map
			.set(map);
		mapped_index
	}

	#[inline(always)]
	/// Get the correctly mapped index without bound checking.
	///
	/// # Safety
	///
	/// - It is undefined behaviour to index outside of a [`slice`]'s bounds.
	///
	/// [`slice`]: std#primitive.slice
	unsafe fn get_index_unchecked(&self, index: usize) -> usize {
		let map = self
			.stream_map
			.take();
		let mapped_index = unsafe { *map.get_unchecked(index) };
		self
			.stream_map
			.set(map);
		mapped_index
	}

	/// Get the nth mapped index's [`Stream`].
	///
	/// # Safety
	///
	/// - This function will return [`None`] if the provided index is out of bounds.
	pub fn nth(&self, index: usize) -> Option<&Track> {
		self
			.get_index(index)
			.map(|index|
				unsafe {
					self
						.streams
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
	/// [`nth`]: Self::nth
	pub fn nth_mut(&mut self, index: usize) -> Option<&mut Track> {
		self
			.get_index(index)
			.map(|index|
				unsafe {
					self
						.streams
						.get_unchecked_mut(index)
				}
			)
	}

	/// Get the nth mapped index's [`Stream`] without bound checking.
	///
	/// # Safety
	///
	/// - It is undefined behaviour to index outside of a [`slice`]'s bounds.
	///
	/// [`slice`]: std#primitive.slice
	pub unsafe fn nth_unchecked(&self, index: usize) -> &Track {
		self
			.streams
			.get_unchecked(self.get_index_unchecked(index))
	}

	/// Mutable counterpart to [`nth_unchecked`]
	///
	/// # Safety
	///
	/// - It is undefined behaviour to index outside of a [`slice`]'s bounds.
	///
	/// [`nth_unchecked`]: Self::nth_unchecked
	/// [`slice`]: std#primitive.slice
	pub unsafe fn nth_unchecked_mut(&mut self, index: usize) -> &mut Track {
		let mapped_index = self.get_index_unchecked(index);
		self
			.streams
			.get_unchecked_mut(mapped_index)
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

impl TryFrom<SerDePlaylist> for Playlist {
	type Error = Error;

	fn try_from(SerDePlaylist { song, time }: SerDePlaylist) -> Result<Self, Error> {
		song
			.into_iter()
			.enumerate()
			.map(|(index, track)| Ok((index, track.try_into()?)))
			.collect::<Result<Vec<(usize, Track)>, Error>>()
			.map(|tuple|
				{
					let (stream_map, streams): (Vec<usize>, Vec<Track>) = tuple
						.into_iter()
						.unzip();
					Self {
						stream_map: Cell::new(stream_map),
						streams,
						repeats: Cell::new(time.unwrap_or_default()),
					}
				}
			)
	}
}

impl Track {
	pub fn play_with(&self, data: &Playhandle) -> Result<ControlFlow, Error> {
		let pointer =  self
			.stream
			.as_slice() as *const [u8];
		data.stream_play(Cursor::new(unsafe { &*pointer }))?;
		let Some(controls) = data
			.io_handle_get()
			.controls_get() else { Err(UnwrapError::InvalidOption)? };

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
					data.playlist_index_set(increment)?;
					return Ok(ControlFlow::SkipSkip)
				},
				Ok(Signal::PlaylistBack) => {
					data.playback_clear();
					clear()?;
					data.playlist_index_set(decrement)?;
					return Ok(ControlFlow::SkipSkip)
				},
				Ok(Signal::Exit) => return Ok(ControlFlow::Break),

				Ok(Signal::TrackNext) => {
					data.playback_clear();
					clear()?;
					data.track_index_set(increment)?;
					return Ok(ControlFlow::Skip)
				},
				Ok(Signal::TrackBack) => {
					data.playback_clear();
					clear()?;
					data.track_index_set(decrement)?;
					return Ok(ControlFlow::Skip)
				},
				Ok(Signal::Play) => data.playback_toggle(),

				Ok(Signal::VolumeIncrease) => data.volume_increment(),
				Ok(Signal::VolumeDecrease) => data.volume_decrement(),
				Ok(Signal::Mute) => data.volume_mute(),

				Err(RecvTimeoutError::Disconnected) => Err(UnwrapError::ChannelDisconnect)?,
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
		data.track_index_set(increment)?;
		Ok(().into())
	}

	pub fn play_headless_with(&self, data: &Playhandle) -> Result<(), Error> {
		let pointer = self
			.stream
			.as_slice() as *const [u8];
		data.stream_play(Cursor::new(unsafe { &*pointer }))?;

		let mut whole_elapsed_time = Duration::ZERO;
		while whole_elapsed_time < self.duration {
			data.player_display(whole_elapsed_time)?;
			sleep(TICK);
			whole_elapsed_time += TICK;
		}
		if self.repeats_can() {
			self.repeats_update();
			return self.play_headless_with(data)
		}
		Ok(())
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

		let mut file = File::open(&path)?;
		let mut stream = Vec::with_capacity(127);
		file.read_to_end(&mut stream)?;

		let duration = read_from_path(path)?
			.properties()
			.duration();
		Ok(
			Self {
				stream,
				repeats: Cell::new(time.unwrap_or_default()),
				duration,
			}
		)
	}
}

impl Playhandle {
	#[inline(always)]
	/// Count the number of [`Streams`] held.
	pub fn entries_count(&self) -> usize {
		self
			.streams_vector
			.len()
	}

	/// Display the default player.
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

	/// Play all streams back.
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

	pub fn all_streams_play_headless(&mut self) -> Result<(), Error> {
		while self
			.playlist_index_check()
			.is_none()
		{
			unsafe {
				let index = self.playlist_index_get_unchecked();
				self
					.streams_vector
					.get_unchecked_mut(index)
					.shuffle();
				self
					.streams_vector
					.get_unchecked(index)
					.play_headless_with(self)?
			}
		}
		Ok(())
	}


	#[inline(always)]
	/// Play a single source back.
	pub fn stream_play(&self, source: impl Read + Seek + Send + Sync + 'static) -> Result<(), Error> {
		self
			.io
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
	fn playlist_index_check(&self) -> Option<VectorError> {
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
	fn track_index_check(&self) -> Option<VectorError> {
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
				.entry_count()
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
	/// - This function corresponds to a basically return the raw held pointer.
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
	/// - This function corresponds to a basically return the raw held pointer.
	pub unsafe fn track_index_get_unchecked(&self) -> usize {
		self
			.current_track_index
			.get()
	}

	/// Reset the track-pointer and set the playlist-pointer.
	pub fn playlist_index_set(&self, setter: impl FnOnce(usize) -> usize) -> Result<(), VectorError> {
		let _ = self.track_index_set(|_| 0);
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
	pub fn track_index_set(&self, setter: impl FnOnce(usize) -> usize) -> Result<(), VectorError> {
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

	pub fn io_handle_get(&self) -> &IOHandle { &self.io }
	pub fn io_handle_take(self) -> IOHandle { self.io }

	#[inline]
	pub fn volume_get(&self) -> f32 {
		self
			.volume
			.get()
			.clamp(0.0, 2.0)
	}

	#[inline]
	pub fn volume_set(&self, map: impl FnOnce(f32) -> f32) {
		self
			.volume
			.set(map(self.volume_get()).clamp(-1.0, 2.0))
	}

	#[inline]
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
	/// Low level mute function.
	///
	/// Call [`set_volume`] to take effect.
	///
	/// [`set_volume`]: Self::set_volume
	pub fn volume_mute(&self) { self.volume_set_raw(|old| old + 2.0 * -old) }

	#[inline]
	/// Low level dial up function.
	///
	/// Counterpart: [`volume_decrement`]
	///
	/// Call [`set_volume`] to take effect.
	///
	/// [`volume_decrement`]: Self::volume_decrement
	/// [`set_volume`]: Self::set_volume
	pub fn volume_increment(&self) { self.volume_set_raw(|old| old + STEP) }

	#[inline]
	/// Low level dial down function.
	///
	/// Counterpart: [`volume_increment`]
	///
	/// Call [`set_volume`] to take effect.
	///
	/// [`volume_increment`]: Self::volume_increment
	/// [`set_volume`]: Self::set_volume
	pub fn volume_decrement(&self) { self.volume_set_raw(|old| old - STEP) }

	/// Update the volume on the internal [`Sink`].
	pub fn volume_update(&self) {
		self
			.io
			.playback_get()
			.set_volume(self.volume_get());
	}

	#[inline(always)]
	pub fn playback_play(&self) {
		self
			.io
			.playback_get()
			.play();
		self
			.paused
			.set(false)
	}

	#[inline(always)]
	pub fn playback_pause(&self) {
		self
			.io
			.playback_get()
			.pause();
		self
			.paused
			.set(true)
	}

	#[inline]
	pub fn playback_toggle(&self) {
		if self.playback_is_paused() { self.playback_play() } else { self.playback_pause() }
	}

	#[inline]
	pub fn playback_clear(&self) {
		self
			.io
			.playback_get()
			.clear()
	}

	#[inline(always)]
	pub fn playback_is_paused(&self) -> bool {
		self
			.paused
			.get()
	}

	pub fn bundle_and_streams_vector_from(bundle: IOHandle, streams_vector: Vec<Playlist>) -> Result<Self, Error> {
		Ok(
			Self {
				current_track_index: Cell::new(0),
				current_playlist_index: Cell::new(0),

				streams_vector,
			
				volume: Cell::new(1.0),
				paused: Cell::new(
					bundle
						.playback_get()
						.is_paused()
				),

				io: bundle,
			}
		)
	}
}

impl From<()> for ControlFlow {
	fn from(_: ()) -> Self { Self::Default }
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
