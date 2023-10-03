///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use rodio::{
	OutputStream,
	OutputStreamHandle,
	PlayError,
};
use serde::Deserialize;
use std::{
	path::PathBuf,
	io::{ BufReader, Write },
	ops::{ Deref, DerefMut },
	fs::{ File, read_to_string },
	thread::{ JoinHandle, spawn },
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
	echo::clear,
	disable_raw_mode,
};
use lofty::{
	read_from_path,
	AudioFile,
	LoftyError,
};
use toml::from_str;
use fastrand::Rng;
use crossbeam_channel::{
	unbounded,
	Sender,
	Receiver,
};
use crossterm::event::{
	self,
	Event,
	KeyEvent,
	KeyCode,
	KeyModifiers,
};
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Global volume of every playback.
pub(crate) static mut VOLUME: f32 = 1.0;
// 1 + 2 * -1 = 1 - 2 = -1 
// -1 + 2 * 1 = -1 + 2 = 1
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

/// The final form of a [`Playlist`]
pub(crate) struct UnwrappedPlaylist {
	name: Box<str>,
	repeats: isize,
	tracks: Box<[UnwrappedTrack]>, // raw for static cast
	track_index: usize,
}

/// The final form of a [`Playlist`]
pub(crate) struct UnwrappedTrack {
	name: Box<str>,
	repeats: isize,
	/// The path to a track's audio file.
	file_path: PathBuf, // is raw for static cast
	/// The exact [`Duration`] of a [`stream`]
	///
	/// [`stream`]: Self#field.stream
	duration: Duration,
}

/// Bundled In- and Output constructs.
///
/// The values, that the structure holds, will be initialised if the program successfully loads at least a single playlist.\
/// This generally means that this type is always contained inside of a wrapper type, that can be uninitialised (e.g: A [`OnceCell`]).
///
/// # Basic usage:
///
/// ```rust
/// # use std::cell::OnceCell;
/// # use crate::in_out::Bundle;
/// #
/// let maybe_bundle = OnceCell::new();
/// /* load stuff */
///
/// let bundle = maybe_bundle.get_or_init(Bundle::new);
/// /* do stuff */
/// ```
/// This example uses a [`OnceCell`].
///
/// [`OnceCell`]: std::cell::OnceCell
pub(crate) struct Bundle {
	sound_out: (OutputStream, OutputStreamHandle), // NOTE(from: OST-Gh): Needs to be tuple, otherwise breaks
	controls: Option<Controls>,
}

/// A wrapper around a thread handle.
///
/// This structure bundles: The control thread handle, a sender, and a receiver.\
/// The sender's purpose is to notify the control thread that it should exit.\
/// On the other hand, the receiver exists in order to receive [`signals`] from the control thread.\
/// Said control thread is responsible for reading keyboard inputs from a, raw mode set, terminal, and parsing them into [`signals`].
///
/// [`signals`]: Signal
pub(crate) struct Controls {
	control_thread: JoinHandle<()>,
	exit_notifier: Sender<()>,
	signal_receiver: Receiver<Signal>,
	// unix_signal_receiver: 
}

create_flags!{
	#[cfg_attr(debug_assertions, derive(Debug))]
	/// A flag bundle.
	///
	/// This structure is used to partially parse the passed in [`program arguments`] for further use.
	///
	/// The position of flags can only directly be after the executable path (e.g.: //usr/local/bin/quing).\
	/// This' made to be that way, due to the fact that the arguments, after the flags, could all be considered file names.\
	/// Flags can be merged, meaning that one does not need to specify multiple separate flags, for example: `quing -h -f`, is instead, `quing -hf`.\
	/// Flag ordering does not matter.
	///
	/// See the associated constants on [`Flags`] for which [`character`] identifies which flag.
	///
	/// [`program arguments`]: std::env::args
	/// [`character`]: char
	[[Flags]]

	/// Wether if the program should create a control-thread, or not.
	should_spawn_headless = 'h'

	/// If the program should merge all given [`Playlists`] into one.
	///
	/// [`Playlists`]: Playlist
	should_flatten = 'f'

	/// Wether or not the file-playlist should repeat infinitely
	should_repeat_playlist = 'p'

	/// When present, will indicate that each file in the file-playlist should reoeat infinitely.
	should_repeat_track = 't'

	/// Wether or not the program should output some information.
	should_print_version = 'v'

	[self.const]
	INUSE_IDENTIFIERS = [..]
	SHIFT = 97
	LENGTH = 26
	CHECK = { |symbol| symbol.is_ascii_lowercase() && symbol.is_ascii_alphabetic() }

	[self.as]
	name = u32

}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(debug_assertions, derive(Debug))]
/// An instruction that the [`play`] function of a [`Playlist`] uses to control itself.
///
/// [`play`]: Playlist::play
pub(crate) enum Instruction {
	/// Exit the program.
	ExitQuit,

	/// Manual skip to the next track.
	NextNone,
	/// Manual backwards skip to a previous track.
	BackNone,

	/// Manual skip to the next playlist.
	NextNext,
	/// Manual backwards skip to a previous playlist.
	BackBack,
	/// Manual instruction to not progress the track pointer.
	HoldNoop,
}

#[cfg_attr(debug_assertions, derive(Debug))]
/// The main controls.
pub(crate) enum Signal {
	/// Corresponds to: `l`.
	TrackNext,
	/// Corresponds to: `j`.
	TrackBack,
	/// Corresponds to: `k`.
	Play,

	/// Corresponds to: `N` (`n` + `shift`).
	PlaylistNext,
	/// Corresponds to: `P` (`p` + `shift`).
	PlaylistBack,
	/// Corresponds to: `k`.
	Exit,

	/// Corresponds to: `up` (up arrow).
	VolumeIncrease,
	/// Corresponds to: `down` (down arrow).
	VolumeDecrease,
	/// Corresponds to: `m`.
	Mute,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[macro_export]
/// Macro that creates a 32-bit flag structure.
macro_rules! create_flags {
	(
		$(#[$structure_attribute: meta])*
		[[$structure: ident]]
		$(
			$(#[$field_attribute: meta])*
			$field: ident = $flag: literal
		)+

		[self.const]
		$set: ident = [..]
		$shift: ident = $by: literal
		$length: ident = $number: literal
		$check: ident = { $($token: tt)+ }

		[self.as]
		name = $type: ty
	) => {
		$(#[$structure_attribute])*
		pub(crate) struct $structure($type);

		impl $structure {
			/// A set made up of each flag identifier.
			const $set: [char; 0 $( + { $flag /* i hate this */; 1 })+] = [$($flag),+];

			const $shift: $type = $by;
			/// The length of the set that contain all possible single character flags.
			const $length: $type = $number;

			/// The current check that determines wether or not a character is valid.
			const $check: fn(&char) -> bool = $($token)+;

			#[inline(always)]
			pub(crate) fn into_inner(self) -> $type { self.0 }

			$(
				#[doc = concat!("Specify using '`-", $flag, "`'.")]
				$(#[$field_attribute])*
				// macro bullshit
				pub(crate) fn $field(&self) -> bool {
					#[cfg(debug_assertions)] if !Self::$check(&$flag) { panic!("get a flag  NOT-ALPHA") }
					**self >> Self::from($flag).into_inner() & 1 == 1 // bit hell:)
					// One copy call needed (**)
					//
					// Six bits unallocated, but twenty six used.
					// 0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0
					//                         z   y   x   w   v   u   t   s   r   q   p   o   n   m   l   k   j   i   h   g   f   e   d   c   b   a
					//                       122 121 120 119 118 117 116 115 114 113 112 111 110 109 108 107 106 105 104 103 102 101 100 099 098 097
					//                       025 024 023 022 021 020 019 018 017 016 015 014 013 012 011 010 009 008 007 006 005 004 003 002 001 000
				}
			)+

		}

	};
}
use create_flags; // shitty workaround
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Implementation utility function for getting a [`Track`]'s or [`Playlist`]'s name.
fn name_from(optional: &Option<Box<str>>) -> Box<str> {
	match optional {
		Some(name) => name.clone(),
		None => unsafe { Box::from_raw("Untitled" as *const str as *mut str) },
	}
}

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

/// Copy the volume [`VOLUME`] currently holds.
pub(crate) fn get_volume() -> f32 { unsafe { VOLUME }.clamp(0.0, 2.0) }

/// Change the volume [`VOLUME`] holds.
pub(crate) fn set_volume<F>(mut new: F)
where
	F: FnMut(f32) -> f32,
{ unsafe { VOLUME = new(VOLUME).clamp(-1.0, 2.0) } } 
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl Playlist {
	/// Load a [`Playlist`] from a [`Path`] represented as a [`String`].
	///
	/// The string is, before being loaded, passed into the [`fmt_path`] function.
	///
	/// [`Path`]: std::path::Path
	pub(crate) fn try_from_contents(contents: String, path: String) -> Option<Self> {
		match from_str(&contents) {
			Ok(playlist) => Some(playlist),
			Err(why) => log!(path; "parsing [{}]" why; None?),
		}
	}

	/// Filter out [`Playlist`] [`files`] from audio [`files`].
	///
	/// [`files`]: std::fs::File
	pub(crate) fn from_paths_with_flags(iterator: impl Iterator<Item = String>, flags: &Flags) -> Vec<Self> {
		let mut lists = Vec::with_capacity(8);
		let time = flags
			.should_repeat_track()
			.then_some(-1);
		let outliers = iterator.fold(
			Playlist {
				name: None,
				song: Vec::with_capacity(8),
				time: flags
					.should_repeat_playlist()
					.then_some(-1)
			},
			|mut playlist, path|
			match read_to_string(fmt_path(&path)) { // might not always work (might sometimes be mp3 but still contain fully valid utf-8 'till the end)
				Ok(contents) => {
					if let Some(new_playlist) = Self::try_from_contents(contents, path) { lists.push(new_playlist) }
					playlist
				},
				Err(why) => {
					log!(path; "loading [{}]" why);
					let boxed = path.into_boxed_str();
					playlist
						.song
						.push(
							Track {
								name: Some(boxed.clone()),
								file: boxed,
								time,
							}
						);
					playlist
				},
			}
		);
		if !outliers.is_empty() { lists.push(outliers) }
		if flags.should_flatten() { lists = vec![Playlist::flatten(lists)] }
		lists
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
					let name = String::from(list.name());
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
	pub(crate) fn name(&self) -> Box<str> { name_from(&self.name) }

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

	/// [`is_empty`] delegate
	///
	/// [`is_empty`]: Vec::is_empty
	pub(crate) fn is_empty(&self) -> bool {
		self
			.song
			.is_empty()
	}
}

impl UnwrappedPlaylist {
	pub(crate) fn play(&mut self, bundle: &Bundle) -> Option<Instruction> {
		let old = self.track_index;
		let length = self
			.tracks
			.len();

		while self.repeats != 0 {
			while self.track_index < length {
				let track = unsafe {
					self
						.tracks
						.get_unchecked_mut(old)
				};
				match track.play(&self.name, bundle) {
					Some(Instruction::BackNone) => self.track_index -= (self.track_index > 0) as usize,
					Some(Instruction::NextNone) => self.track_index += 1,
					Some(Instruction::HoldNoop) => { },
					Some(handled_by_outer) => return Some(handled_by_outer),
					None => if track.repeats != 0 {
						track.repeats -= 1;
						continue
					} else { self.track_index += 1 },
				}
			}
			if self.repeats != 0 {
				self.repeats -= 1;
				continue
			}
		}
		None
	}

	/// [`is_empty`] delegate
	///
	/// [`is_empty`]: Vec::is_empty
	pub(crate) fn is_empty(&self) -> bool {
		self
			.tracks
			.is_empty()
	}
}

impl TryFrom<&Playlist> for UnwrappedPlaylist {
	type Error = (Box<str>, LoftyError);

	/// Load each [`Track`]'s duration and stream.
	///
	/// The function's output will be put into the global variable [`FILES`].\
	/// This function also clears [`FILES`] when it successfully loads at least one [`Track`].
	fn try_from(playlist: &Playlist) -> Result<Self, Self::Error> {
		let repeats = playlist
			.time
			.unwrap_or_default();
		let name = playlist.name();
		Ok(
			Self {
				name,
				repeats,
				tracks: playlist
					.song
					.iter()
					.map(UnwrappedTrack::try_from)
					.collect::<Result<Vec<UnwrappedTrack>, Self::Error>>()?
					.into_boxed_slice(),
				track_index: 0,
			}
		)
	}
}

impl Track {
	/// Get the name of the passed in [`Track`].
	///
	/// If the playlist's name is set to [`None`], the function will return the [`string slice`] `"Untitled"`.
	///
	/// [`string slice`]: str
	pub(crate) fn name(&self) -> Box<str> { name_from(&self.name) }
}

impl UnwrappedTrack {
	pub(crate) fn play(&mut self, playlist_name: &Box<str>, bundle: &Bundle) -> Option<Instruction> {
		let handle = match bundle.play_file(self.open()) {
			Ok(playback) => playback,
			Err(why) => log!(self.name; "starting to play [{}]" why; return Some(Instruction::NextNone)),
		};
		handle.set_volume(get_volume());
		let Some(controls) = bundle.get_controls() else {
			handle.sleep_until_end();
			None?
		};

		let mut elapsed = Duration::ZERO;

		while elapsed <= self.duration {
			let paused = handle.is_paused();

			print!("\r[{playlist_name}][{}][{}][{:>5.2}]\0",
				self.name,
				{
					let seconds = elapsed.as_secs();
					let minutes = seconds / 60;
					format!("{:0>2}:{:0>2}:{:0>2}", minutes / 60, minutes % 60, seconds % 60)
				},
				get_volume(),
			);
			if let Err(why) = stdout().flush() { log!(; "flushing" why) }

			let now = Instant::now(); // if pc's closed does still count??
			let under_threshhold = elapsed <= Duration::from_secs(2);
			let is_paused = handle.is_paused();
			let update_volume = |setter: fn(f32) -> f32|
			{
				set_volume(setter);
				handle.set_volume(get_volume());
				now.elapsed()
			};
			elapsed += match controls.receive_signal(now) {
				Err(RecvTimeoutError::Timeout) => if paused { continue } else { TICK },
				Ok(Signal::PlaylistNext) => return Some(Instruction::NextNext),
				Ok(Signal::PlaylistBack) => if under_threshhold { return Some(Instruction::BackBack) } else { return Some(Instruction::HoldNoop) },
				Ok(Signal::Exit) => {
					clear();
					return Some(Instruction::ExitQuit)
				},

				Ok(Signal::TrackNext) => return Some(Instruction::NextNone),
				Ok(Signal::TrackBack) => if under_threshhold { return Some(Instruction::BackNone) } else { return Some(Instruction::HoldNoop) }
				Ok(Signal::Play) => {
					if is_paused { handle.play() } else { handle.pause() }
					now.elapsed()
				},

				Ok(_) if is_paused => { continue }, // guard against updating volume whilst paused
				Ok(Signal::VolumeIncrease) => update_volume(|old| old + 0.05),
				Ok(Signal::VolumeDecrease) => update_volume(|old| old - 0.05),
				Ok(Signal::Mute) => update_volume(|old| old + 2.0 * -old),

				Err(RecvTimeoutError::Disconnected) => {
					log!(; "receiving control-thread" DISCONNECTED);
					return Some(Instruction::ExitQuit)
				}, // chain reaction will follow
			}
		}
		None
	}

	/// Assumes the file from the [`try_from`] call still is at the same location, and opens it.
	pub(crate) fn open(&self) -> BufReader<File> { BufReader::new(File::open(&self.file_path).unwrap()) }
}


impl TryFrom<&Track> for UnwrappedTrack {
	type Error = (Box<str>, LoftyError);
	
	fn try_from(track: &Track) -> Result<Self, Self::Error> {
		let repeats = track
			.time
			.unwrap_or_default();
		let name = track.name();
		
		let file_path = fmt_path(&track.file);

		let duration = match read_from_path(&file_path) {
			Ok(info) => info
				.properties()
				.duration(),
			Err(lofty_error) => return Err((name, lofty_error)),
		};

		Ok(
			Self {
				name,
				repeats,
				file_path,
				duration,
			}
		)
	}
}

impl Bundle {
	/// Create a new [`Bundle`].
	///
	/// # Input value beased behavior:
	///
	/// - [`true`]: Spawns the control thread without checking if it should or not.
	/// - [`false`]: [`true`]'s opposite, it does not spawn it.
	///
	/// [`with`]: Self::with
	pub(crate) fn with(is_tty: bool) -> Self {
		let sound_out = rodio::OutputStream::try_default().unwrap_or_else(|why|
			{
				if let Err(why) = disable_raw_mode() { log!(; "disabling raw-mode" why) }
				panic!("determine the default audio output device  {why}")
			}
		);

		let (signal_sender, signal_receiver) = unbounded();
		let (exit_notifier, exit_receiver) = unbounded();

		Self {
			sound_out,
			controls: is_tty.then_some(
				Controls {
					control_thread: spawn(move ||
						while let Err(RecvTimeoutError::Timeout) = exit_receiver.recv_timeout(TICK) {
							if !event::poll(TICK).unwrap_or_else(|why| panic!("poll an event from the current terminal  {why}")) { continue }
							let signal = match event::read().unwrap_or_else(|why| panic!("read an event from the current terminal  {why}")) {
								Event::Key(KeyEvent { code: KeyCode::Char('n' | 'N'), .. }) => Signal::PlaylistNext,
								Event::Key(KeyEvent { code: KeyCode::Char('p' | 'P'), .. }) => Signal::PlaylistBack,
								Event::Key(KeyEvent { code: KeyCode::Char('c' | 'C'), modifiers, .. }) if modifiers.contains(KeyModifiers::CONTROL) => return if let Err(why) = signal_sender.send(Signal::Exit) { log!(; "sending a signal" why) },
								Event::Key(KeyEvent { code: KeyCode::Esc, ..  }) => return if let Err(why) = signal_sender.send(Signal::Exit) { log!(; "sending a signal" why) },

								Event::Key(KeyEvent { code: KeyCode::Char('l' | 'L') | KeyCode::Right, .. }) => Signal::TrackNext,
								Event::Key(KeyEvent { code: KeyCode::Char('j' | 'J') | KeyCode::Left, .. }) => Signal::TrackBack,
								Event::Key(KeyEvent { code: KeyCode::Char('k' | 'K' | ' '), .. }) => Signal::Play,

								Event::Key(KeyEvent { code: KeyCode::Char('m' | 'M'), .. }) => Signal::Mute,
								Event::Key(KeyEvent { code: KeyCode::Up, .. }) => Signal::VolumeIncrease,
								Event::Key(KeyEvent { code: KeyCode::Down, .. }) => Signal::VolumeDecrease,

								Event::FocusGained | Event::FocusLost => Signal::Play,
								_ => continue,
							};
							if let Err(_) = signal_sender.send(signal) { panic!("send a signal to the playback  {DISCONNECTED}") }
						}
					),
					exit_notifier,
					signal_receiver,
				}
			),
		}
	}

	/// Get a reference to the underlying control structure.
	pub(crate) fn get_controls(&self) -> Option<&Controls> {
		self
			.controls
			.as_ref()
	}

	/// Take the underlying controls.
	pub(crate) fn take_controls(self) -> Option<Controls> {
		self
			.controls
	}

	/// Play a single file.
	pub(crate) fn play_file(&self, song: BufReader<File>) -> Result<Sink, PlayError> {
		self
			.sound_out
			.1
			.play_once(song)
	}
}

impl Controls {
	/// Clean up a (hopefully done) control thread.
	///
	/// Supposed to be used in conjunction with [`notify_exit`].
	///
	/// # Basic usage:
	///
	/// ```rust
	/// # use crate::in_out::Bundle;
	/// #
	/// let bundle = Bundle::new();
	/// /* do stuff */
	///
	/// if let Some(controls) = bundle.take_controls() {
	///     controls.notify_exit();
	///     controls.clean_up()
	/// }
	/// ```
	/// Used things: [`notify_exit`], [`Bundle`], and [`take_controls`].
	///
	/// [`notify_exit`]: Self::notify_exit
	/// [`take_controls`]: Bundle::take_controls
	pub(crate) fn clean_up(self) {
		let _ = self
			.control_thread
			.join();
	}


	/// Notify the control thread to exit if it hasn't already.
	///
	/// # Basig usage:
	///
	/// ```rust
	/// # use crate::in_out::Bundle;
	/// #
	/// let bundle = Bundle::new();
	/// /* do stuff */
	///
	/// if let Some(control_reference) = bundle.get_controls() { control_refernce.notify_exit() }
	/// ```
	/// Used things: [`Bundle`], and [`get_controls`].
	///
	/// [`get_controls`]: Bundle::get_controls
	pub(crate) fn notify_exit(&self) {
		let _ = self
			.exit_notifier
			.send(());
	}

	/// Try to receive a signal by waiting for it for a set amount of time.
	pub(crate) fn receive_signal(&self, moment: Instant) -> Result<Signal, RecvTimeoutError> {
		self
			.signal_receiver
			.recv_deadline(moment + TICK)
	}
}

impl Flags {
	/// Split the program arguments into files and flags.
	///
	/// # Panics:
	///
	/// - Arguments are empty.
	pub(crate) fn separate_from(iterator: Vec<String>) -> (Self, impl Iterator<Item = String>) {
		let mut flag_count = 0;
		let bits = iterator
			.iter()
			.map_while(|argument|
				{
					let raw = argument
						.strip_prefix('-')?
						.replace(|symbol| !Self::CHECK(&symbol), "");
					flag_count += 1;
					Some(raw)
				}
			)
			.fold(
				Self(0),
				|mut bits, raw|
				{
					for symbol in raw
						.chars()
						.filter(|symbol| Self::INUSE_IDENTIFIERS.contains(symbol))
					{ *bits |= 1 << Self::from(symbol).into_inner() }
					bits
				}
			);
		(
			bits,
			iterator
				.into_iter()
				.skip(flag_count)
		)
	}
}

impl From<char> for Flags {
	fn from(symbol: char) -> Self {
		#[cfg(debug_assertions)] if !Self::CHECK(&symbol) { panic!("get a flag  NOT-ALPHA") }
		Self((symbol as u32 - Self::SHIFT) % Self::LENGTH)
	}
}

impl Deref for Flags {
	type Target = u32;

	#[inline(always)]
	fn deref(&self) -> &u32 { &self.0 }
}

impl DerefMut for Flags {
	#[inline(always)]
	fn deref_mut(&mut self) -> &mut u32 { &mut self.0 }
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
