///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use lofty::{
	AudioFile,
	read_from_path,
};
use rodio::Sink;
use std::{
	path::Path,
	fs::File,
	time::Duration,
	// mem::MaybeUninit,
	// ptr::NonNull,
	io::{
		BufReader,
		Cursor,
		Read,
	},
};
use fastrand::Rng;
use super::Error;
use super::VectorError;
use super::songs::{ Track, Playlist };
use super::in_out::IOHandle;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
const STEP: f32 = 0.05;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
type Handle = BufReader<File>;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct Streams {
	/// Map of indexes that map directly to the vector of [`streams`]
	///
	/// [`streams`]: Self#field.streams
	stream_map: Vec<usize>,
	streams: Vec<Stream>,
	repeats: isize,
}

#[cfg_attr(debug_assertions, derive(Debug))]
pub struct Stream {
	stream: Vec<u8>,
	repeats: isize,
	duration: Duration,
}

pub struct Playhandle<'a> {
	current_track_index: usize,
	current_playlist_index: usize,

	streams_vector: Vec<Streams>,

	/// Global volume.
	volume: f32,
	paused: bool,
	//  1.0 + 2.0 * -1.0 = -1.0
	// -1.0 + 2.0 *  1.0 =  1.0

	io: &'a IOHandle,
	sink: Sink,

	generator: Rng,
	// phantomdata
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(debug_assertions, derive(Debug))]
#[derive(Default)]
pub enum ControlFlow {
	Break,
	#[default] Default,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl Streams {
	pub fn play_with(&mut self, handle: &Playhandle) -> Result<(), Error> {
		let Some(mut stream) = self.nth_mut(handle.current_track_index) else { Err(VectorError::OutOfBounds)? };
		stream.play_with(handle)
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
		self
			.streams
			.get_unchecked_mut(self.get_index_unchecked(index))
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
						repeats: time.unwrap_or_default(),
					}
				}
			)
	}
}

impl Stream {
	pub fn play_with(&self, data: &Playhandle) -> Result<(), Error> {
		let sink = data
			.io
			.play_stream(
				Cursor::new(
					self
						.stream
						.as_slice()
				)
			)?;
		Ok(())
	}
}

impl TryFrom<Track> for Stream {
	type Error = Error;

	fn try_from(Track { file, time }: Track) -> Result<Self, Error> {
		let path = Path::new(&file.into_string());

		let mut file = File::open(path)?;
		let mut stream = Vec::with_capacity(127);
		file.read_to_end(&mut stream)?;

		let duration = read_from_path(path)?
			.properties()
			.duration();
		Ok(
			Self {
				stream,
				repeats: time.unwrap_or_default(),
				duration,
			}
		)
	}
}

impl<'a> Playhandle<'a> {
	const STEP: f32 = 0.05;

	pub fn play(&mut self) -> Result<(), Error>  {
		while self.current_playlist_index < self
			.streams_vector
			.len()
		{
			let Some(mut streams) = self
				.streams_vector
				.get_mut(self.current_playlist_index) else { Err(VectorError::OutOfBounds)? };
			streams.shuffle(&mut self.generator);
			streams.play_with(&self);
			self.current_playlist_index += 1
		}
		Ok(())
	}

	pub fn play_playlist(&mut self, playlist: Playlist) -> Result<(), Error> {
		Ok(())
	}

	const fn clamp_current(&mut self) {
		let Some(maximum) = self
			.streams_vector
			.get(self.current_playlist_index)
			.map(|vector|
				vector
					.streams
					.len()
			) else { return };
		self
			.current_track_index = self
			.current_track_index
			.clamp(0, maximum);
	}

	#[inline(always)]
	const fn clamp_volume_internal(&mut self) {
		self.volume = self
			.volume
			.clamp(-1.0, 2.0)
	}

	pub fn get_volume(&self) -> f32 {
		self
			.volume
			.clamp(0.0, 2.0)
	}

	#[inline]
	/// Low level mute function.
	///
	/// Call [`set_volume`] to take effect.
	///
	/// [`set_volume`]: Self::set_volume
	pub const fn volume_mute(&mut self) { self.volume += 2.0 * -self.volume }

	#[inline]
	/// Low level dial up function.
	///
	/// Counterpart: [`volume_decrement`]
	///
	/// Call [`set_volume`] to take effect.
	///
	/// [`volume_decrement`]: Self::volume_decrement
	/// [`set_volume`]: Self::set_volume
	pub const fn volume_increment(&mut self) {
		self.volume += Self::STEP;
		self.clamp_volume_internal()
	}

	#[inline]
	/// Low level dial down function.
	///
	/// Counterpart: [`volume_increment`]
	///
	/// Call [`set_volume`] to take effect.
	///
	/// [`volume_increment`]: Self::volume_increment
	/// [`set_volume`]: Self::set_volume
	pub const fn volume_decrement(&mut self) {
		self.volume -= Self::STEP;
		self.clamp_volume_internal()
	}

	/// Update the volume on the internal [`Sink`].
	pub fn set_volume(&mut self) {
		self
			.sink
			.set_volume(self.get_volume());
	}

	pub fn from_bundle_and_streams_vector(bundle: &'a IOHandle, streams_vector: Vec<Streams>) -> Result<Self, Error> {
		Ok(
			Self {
				current_track_index: 0,
				current_playlist_index: 0,

				streams_vector,
			
				volume: 1.0,
				paused: false,

				io: bundle,

				sink: Sink::try_new(bundle.get_sound_out_handle())?,

				generator: Rng::new(),
			}
		)
	}
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
