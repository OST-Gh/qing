///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use rodio::{
	OutputStream,
	OutputStreamHandle,
	PlayError,
};
use crossbeam_channel::{
	self as channel,
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
use std::{
	fs::File,
	io::BufReader,
	thread::{ self, JoinHandle },
	ops::{ Deref, DerefMut },
};
use crate::{
	TICK,
	DISCONNECTED,
	Sink,
	Duration,
	Instant,
	RecvTimeoutError,
	songs::Instruction,
	log,
	clear,
};
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Bundled In- and Output constructs.
///
/// The values, that the structure holds, will be initialised if the program successfully loads at least a single playlist.\
/// This generally means that this type is always contained inside of a wrapper type, that can be uninitialised (e.g: A [`OnceCell`]).
///
/// # Basic usage:
///
/// ```rust
///#use std::cell::OnceCell;
///#use crate::in_out::Bundle;
///
/// let maybe_bundle = OnceCell::new();
/// /* load stuff */
///
/// let bundle = bundle.get_or_init(Bundle::new);
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
	/// [`Playlists`]: crate::songs::Playlist
	should_flatten = 'f'

	/// Wether or not the file-playlist should repeat infinitely
	should_repeat_playlist = 'p'

	/// When present, will indicate that each file in the file-playlist should reoeat infinitely.
	should_repeat_track = 't'

	/// Wether or not the program should output some information.
	should_print_version = 'v'

	[const]
	/// A set made up of each flag identifier.
	INUSE_IDENTIFIERS = [..]
	/// The starting position of the allowed ASCII character range.
	SHIFT = 97 // minimum position in ascii
	/// The length of the set that contain all possible single character flags.
	LENGTH = 26
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(debug_assertions, derive(Debug))]
/// High level control signal representation.
pub(crate) enum Signal {
	PlaylistNext,
	PlaylistBack,
	Exit,

	TrackNext,
	TrackBack,
	Play,

	VolumeIncrease,
	VolumeDecrease,
	Mute,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[macro_export]
macro_rules! count {
	($thing: expr) => { 1 };
	($($thing: expr),* $(,)?) => { 0 $(+ $crate::count!($thing))* };
}
use count; // shitty workaround

#[macro_export]
/// Macro that creates the [`Flags`] structure.
macro_rules! create_flags {
	(
		$(#[$structure_attribute: meta])* [[$structure: ident]]
		$($(#[$field_attribute: meta])* $field: ident = $flag: literal)+

		[const]
		$(#[$lone_attribute: meta])* $lone: ident = [..]
		$(#[$shift_attribute: meta])* $shift: ident = $by: literal
		$(#[$length_attribute: meta])* $length: ident = $number: literal
	) => {
		$(#[$structure_attribute])*
		pub(crate) struct $structure(u32);

		impl $structure {
			$(#[$lone_attribute])* const $lone: [char; count!($($flag),+)] = [$($flag),+];
			$(#[$shift_attribute])* const $shift: u32 = $by;
			$(#[$length_attribute])* const $length: u32 = $number;
			$(
				#[doc = concat!("Specify using '`-", $flag, "`'.")]
				$(#[$field_attribute])*
				// macro bullshit
				pub(crate) fn $field(&self) -> bool {
					#[cfg(debug_assertions)] if !flag_check(&$flag) { panic!("get a flag  NOT-ALPHA") }
					**self >> Self::from($flag).into_inner() & 1 == 1 // bit hell:)
					// One copy call needed (**)
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
/// The current check that determines wether or not a character is valid.
fn flag_check(symbol: &char) -> bool { symbol.is_ascii_alphabetic() && symbol.is_ascii_lowercase() }
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl Bundle {
	/// Create a new [`Bundle`] with an optional control-thread.
	pub(crate) fn with(is_tty: bool) -> Self {
		let sound_out = rodio::OutputStream::try_default().unwrap_or_else(|why| panic!("determine the default audio output device  {why}"));

		let controls = is_tty.then(||
			{
				let (signal_sender, signal_receiver) = channel::unbounded();
				let (exit_notifier, exit_receiver) = channel::unbounded();
				Controls {
					control_thread: thread::spawn(move ||
						while let Err(RecvTimeoutError::Timeout) = exit_receiver.recv_timeout(TICK) {
							if !event::poll(TICK).unwrap_or_else(|why| panic!("poll an event from the current terminal  {why}")) { continue }
							let signal = match event::read().unwrap_or_else(|why| panic!("read an event from the current terminal  {why}")) {
								Event::Key(KeyEvent { code: KeyCode::Char('l' | 'L'), modifiers, .. }) if modifiers.contains(KeyModifiers::CONTROL) => Signal::PlaylistNext,
								Event::Key(KeyEvent { code: KeyCode::Char('j' | 'J'), modifiers, .. }) if modifiers.contains(KeyModifiers::CONTROL) => Signal::PlaylistBack,
								Event::Key(KeyEvent { code: KeyCode::Char('k' | 'K'), modifiers, .. }) if modifiers.contains(KeyModifiers::CONTROL) => return if let Err(why) = signal_sender.send(Signal::Exit) { log!(; "sending a signal" why) },

								Event::Key(KeyEvent { code: KeyCode::Char('l'), ..}) => Signal::TrackNext,
								Event::Key(KeyEvent { code: KeyCode::Char('j'), ..}) => Signal::TrackBack,
								Event::Key(KeyEvent { code: KeyCode::Char('k'), ..}) => Signal::Play,

								Event::Key(KeyEvent { code: KeyCode::Char('L'), .. }) => Signal::VolumeIncrease,
								Event::Key(KeyEvent { code: KeyCode::Char('J'), .. }) => Signal::VolumeDecrease,
								Event::Key(KeyEvent { code: KeyCode::Char('K'), .. }) => Signal::Mute,

								_ => continue,
							};
							if let Err(_) = signal_sender.send(signal) { panic!("send a signal to the playback  {DISCONNECTED}") }
						}
					),
					exit_notifier,
					signal_receiver,
				}
			}
		);

		Self {
			sound_out,
			controls,
		}
	}

	/// Get a reference to the underlying control structure.
	pub(crate) fn get_controls(&self) -> Option<&Controls> {
		self
			.controls
			.as_ref()
	}

	/// Take the underlying controls.
	pub(crate) fn take_controls(self) -> Option<Controls> { self.controls }

	/// Play a single file.
	pub(crate) fn play_file(&self, song: &'static mut BufReader<File>) -> Result<Sink, PlayError> {
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
	///#use crate::in_out::Bundle;
	///
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
	///#use crate::in_out::Bundle;
	///
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

	/// Try to receive a signal, by waiting for it for a set amount of time.
	pub(crate) fn receive_signal(&self, moment: Instant) -> Result<Signal, RecvTimeoutError> {
		self
			.signal_receiver
			.recv_deadline(moment + TICK)
	}
}

impl Flags {
	#[inline(always)]
	/// Get the underlying unsigned integer.
	pub(crate) fn into_inner(self) -> u32 { self.0 }

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
						.replace(|symbol| !flag_check(&symbol), "");
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
		#[cfg(debug_assertions)] if !flag_check(&symbol) { panic!("get a flag  NOT-ALPHA") }
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
// impl Control {
// 	/// Manage the playlist's playback or program.
// 	pub(crate) fn manage(self, elapsed: Duration) -> Instruction {
// 		match self.0 {
// 			Signal::Increment => Instruction::Next,
// 			Signal::Decrement => if elapsed <= Duration::from_secs(1) { return Instruction::Back } else { return Instruction::Hold },
// 			Signal::Toggle => {
// 				clear();
// 				Instruction::Exit
// 			},
// 		}
// 	}
// }

// impl Other {
// 	/// Manage the track's playback.
// 	/// 
// 	/// # Values:
// 	/// - [`true`]: It signals that the track-loop should return a [`Hold`] [`Instruction`].
// 	/// - [`false`]: It signifies the exact opposite.
// 	///
// 	/// [`Hold`]: crate::songs::Instruction::Hold
// 	pub(crate) fn manage(self, playback: &Sink, elapsed: Duration, songs_index: &mut usize) -> bool {
// 		match self.0 {
// 			Signal::Increment => *songs_index += 1,
// 			Signal::Decrement => *songs_index -= (*songs_index > 0 && elapsed <= Duration::from_secs(1)) as usize,

// 			Signal::Toggle => {
// 				if playback.is_paused() { playback.play() } else { playback.pause() }
// 				return false
// 			},
// 		}
// 		true
// 	}
// }

// impl Shift {
// 	/// Manage the program's volume.
// 	pub(crate) fn manage(self, playback: &Sink, now: Instant, volume: &mut f32) -> Duration {
// 		match self.0 {
// 			Signal::Increment => *volume += 0.05,
// 			Signal::Decrement => *volume -= 0.05,
// 			Signal::Toggle => *volume += 2.0 * -*volume,
// 		}
// 		*volume = volume.clamp(-1.0, 2.0);
// 		playback.set_volume(volume.clamp(0.0, 2.0));
// 		if playback.is_paused() { return Duration::ZERO }
// 		now.elapsed()
// 	}
// }
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
