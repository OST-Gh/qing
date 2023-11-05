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
use super::Error;
use super::{ VectorError, UnwrapError };
use super::songs::{ Track, Playlist };
use super::in_out::{ IOHandle, Signal };
use super::utilities::{ clear, fmt_path };
use super::TICK;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
const STEP: f32 = 0.05;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct Streams {
	/// Map of indexes that map directly to the vector of [`streams`]
	///
	/// [`streams`]: Self#field.streams
	stream_map: Vec<usize>,
	streams: Vec<Stream>,
	repeats: Cell<isize>,
}

#[cfg_attr(debug_assertions, derive(Debug))]
pub struct Stream {
	stream: Vec<u8>,
	repeats: Cell<isize>,
	duration: Duration,
}

/// The player's state.
pub struct Playhandle {
	current_coordinates_index: Cell<usize>,
	current_playlist_index: Cell<usize>,

	streams_vector: Vec<Streams>,

	/// Global volume.
	volume: Cell<f32>,
	paused: Cell<bool>,
	//  1.0 + 2.0 * -1.0 = -1.0
	// -1.0 + 2.0 *  1.0 =  1.0

	io: IOHandle,

	generator: Rng,
	// phantomdata
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
impl Streams {
	#[inline(always)]
	pub fn entry_count(&self) -> usize {
		self
			.streams
			.len()
	}

	pub fn play_with(&self, handle: &Playhandle) -> Result<ControlFlow, Error> {
		while handle
			.track_index_get()
			.is_ok()
		{
			match unsafe {
				self
					.nth_unchecked(handle.track_index_get_unchecked())
					.play_with(handle)?
			} {
				ControlFlow::Break => return Ok(ControlFlow::Break),
				ControlFlow::SkipSkip => return Ok(ControlFlow::Skip),
				ControlFlow::Skip => continue,
				ControlFlow::Default => { },
			}
		}
		if self.repeats_can() {
			self.repeats_update();
			return self.play_with(handle)
		}
		Ok(().into())
	}

	pub fn shuffle(&mut self, random_state: &mut Rng) {
		let map = &mut self.stream_map;
		let length = map.len();

		let mut generator = Rng::with_seed(random_state.u64(..));
		generator.shuffle(map);

		for value in 0..length {
			let index = value % length;
			map.swap(index, generator.usize(0..=index));
			map.swap(index, generator.usize(index..length));
			// a b c; b inclusive in both random ranges
			// b a c
			// b c a
		}
	}

	#[inline(always)]
	/// Get the correctly mapped index.
	///
	/// # Safety:
	///
	/// This function will return [`None`] if the provided index is out of bounds.
	fn get_index(&self, index: usize) -> Option<usize> {
		self
			.stream_map
			.get(index)
			.map(usize::clone)
	}

	#[inline(always)]
	/// Get the correctly mapped index without bound checking.
	///
	/// # Safety
	///
	/// It is undefined behaviour to index outside of a [`slice`]'s bounds.
	///
	/// [`slice`]: std#primitive.slice
	unsafe fn get_index_unchecked(&self, index: usize) -> usize {
		*self
			.stream_map
			.get_unchecked(index)
	}

	/// Get the nth mapped index's [`Stream`].
	///
	/// # Safety:
	///
	/// This function will return [`None`] if the provided index is out of bounds.
	pub fn nth(&self, index: usize) -> Option<&Stream> {
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
	/// # Safety:
	///
	/// This function will return [`None`] if the provided index is out of bounds.
	///
	/// [`nth`]: Self::nth
	pub fn nth_mut(&mut self, index: usize) -> Option<&mut Stream> {
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
	/// It is undefined behaviour to index outside of a [`slice`]'s bounds.
	///
	/// [`slice`]: std#primitive.slice
	pub unsafe fn nth_unchecked(&self, index: usize) -> &Stream {
		self
			.streams
			.get_unchecked(self.get_index_unchecked(index))
	}

	/// Mutable counterpart to [`nth_unchecked`]
	///
	/// # Safety
	///
	/// It is undefined behaviour to index outside of a [`slice`]'s bounds.
	///
	/// [`nth_unchecked`]: Self::nth_unchecked
	/// [`slice`]: std#primitive.slice
	pub unsafe fn nth_unchecked_mut(&mut self, index: usize) -> &mut Stream {
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

impl TryFrom<Playlist> for Streams {
	type Error = Error;

	fn try_from(Playlist { song, time }: Playlist) -> Result<Self, Error> {
		song
			.into_iter()
			.enumerate()
			.map(|(index, track)| Ok((index, track.try_into()?)))
			.collect::<Result<Vec<(usize, Stream)>, Error>>()
			.map(|tuple|
				{
					let (stream_map, streams): (Vec<usize>, Vec<Stream>) = tuple
						.into_iter()
						.unzip();
					Self {
						stream_map,
						streams,
						repeats: Cell::new(time.unwrap_or_default()),
					}
				}
			)
	}
}

impl Stream {
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
					data.playlist_index_set(&increment)?;
					data.playback_clear();
					return Ok(ControlFlow::SkipSkip)
				},
				Ok(Signal::PlaylistBack) => {
					data.playlist_index_set(&decrement)?;
					data.playback_clear();
					return Ok(ControlFlow::SkipSkip)
				},
				Ok(Signal::Exit) => return Ok(ControlFlow::Break),

				Ok(Signal::TrackNext) => {
					data.track_index_set(&increment)?;
					data.playback_clear();
					return Ok(ControlFlow::Skip)
				},
				Ok(Signal::TrackBack) => {
					data.track_index_set(&decrement)?;
					data.playback_clear();
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

impl TryFrom<Track> for Stream {
	type Error = Error;

	fn try_from(Track { file, time }: Track) -> Result<Self, Error> {
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
			.playlist_index_get()
			.is_ok()
		{
			let index = self.playlist_index_get()?;
			unsafe {
				self
					.streams_vector
					.get_unchecked_mut(index)
					.shuffle(&mut self.generator)
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
				ControlFlow::Default => self.playlist_index_set(|old| old + 1)?,
			}
			clear()?
		}
		Ok(().into())
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
			.current_coordinates_index
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
				Ok(
					self
						.current_playlist_index
						.get()
				),
				Err,
			)
	}
	/// Get the track-pointer.
	pub fn track_index_get(&self) -> Result<usize, VectorError> {
		self
			.playlist_index_check()
			.map_or_else(
				||
				Ok(self.track_index_get_unchecked()),
				Err,
			)
	}

	#[inline(always)]
	pub fn playlist_index_get_unchecked(&self) -> usize {
		self
			.current_playlist_index
			.get()
	}
	#[inline(always)]
	pub fn track_index_get_unchecked(&self) -> usize {
		self
			.current_coordinates_index
			.get()
	}

	pub fn playlist_index_set(&self, setter: impl FnOnce(usize) -> usize) -> Result<(), VectorError> {
		self
			.playlist_index_check()
			.map_or_else(
				||
				Ok(
					self
						.current_playlist_index
						.set(setter(self.playlist_index_get_unchecked()))
				),
				Err,
			)
	}
	pub fn track_index_set(&self, setter: impl FnOnce(usize) -> usize) -> Result<(), VectorError> {
		self
			.track_index_check()
			.map_or_else(
				||
				Ok(
					self
						.current_coordinates_index
						.set(setter(self.track_index_get_unchecked()))
				),
				Err,
			)
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

	pub fn bundle_and_streams_vector_from(bundle: IOHandle, streams_vector: Vec<Streams>) -> Result<Self, Error> {
		Ok(
			Self {
				current_coordinates_index: Cell::new(0),
				current_playlist_index: Cell::new(0),

				streams_vector,
			
				volume: Cell::new(1.0),
				paused: Cell::new(
					bundle
						.playback_get()
						.is_paused()
				),

				io: bundle,

				generator: Rng::new(),
			}
		)
	}
}

impl From<()> for ControlFlow {
	fn from(_: ()) -> Self { Self::Default }
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
