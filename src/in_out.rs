///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use rodio::{
	OutputStream,
	OutputStreamHandle,
	PlayError,
};
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
use std::{
	collections::HashSet,
	io::BufReader,
	fs::File,
	env::args,
	thread::{ Builder, JoinHandle },
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
	disable_raw_mode,
	echo::clear,
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
	signal_receiver: Receiver<Layer>,
}

#[cfg_attr(debug_assertions, derive(Debug))]
/// [`Signal`] interpretation with `CTRL` held down.
pub(crate) struct Control(Signal);

#[cfg_attr(debug_assertions, derive(Debug))]
/// [`Signal`] interpretation with nothing held down.
pub(crate) struct Other(Signal);

#[cfg_attr(debug_assertions, derive(Debug))]
/// [`Signal`] interpretation with `Shift` held down.
pub(crate) struct Shift(Signal);

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
	/// [`program arguments`]: args
	/// [`character`]: char
	[[Flags]]

	/// Wether if the program should create a control-thread, or not.
	should_spawn_headless = 'h'

	/// If the program should merge all given [`Playlists`] into one.
	///
	/// [`Playlists`]: crate::songs::Playlist
	should_flatten = 'f'

	/// Wether or not the program should output some information.
	should_print_version = 'v'

	[IDENTIFIERS]
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(debug_assertions, derive(Debug))]
/// High level control signal representation.
pub(crate) enum Layer {
	Playlist(Control),
	// Toggle = ProgramExit.
	// Increm = PlaylistNext.
	// Decrem = PlaylistBack.

	Track(Other),
	// Toggle = PlaybackToggle,
	// Increm = TrackNext,
	// Decrem = TrackBack,

	Volume(Shift),
	// Toggle = VolumeIncrease,
	// Increm = VolumeDecrease,
	// Decrem = VolumeToggle,
}

#[cfg_attr(debug_assertions, derive(Debug))]
/// The three main controls.
///
/// A signal can be interpreted alone, but then some meaning would be lost.\
/// See: [`Control`], [`Other`] and [`Shift`].
pub(crate) enum Signal {
	/// Move up within something, usually a manipulate a number.
	///
	/// Corresponds to: `l`.
	Increment,
	/// Move down within something, usually a manipulate a number.
	///
	/// Corresponds to: `j`.
	Decrement,
	/// Toggle something, or perform some kind of special action.
	///
	/// Corresponds to: `k`.
	Toggle,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[macro_export]
/// Macro that creates the [`Flags`] structure.
macro_rules! create_flags {
	($(#[$structure_attribute: meta])* [[$structure: ident]] $($(#[$field_attribute: meta])* $field: ident = $flag: literal)+ [$lone: ident]) => {
		$(#[$structure_attribute])*
		pub(crate) struct $structure {
			$(
				$(#[$field_attribute])*
				///
				#[doc = concat!("Specify using '`-", $flag, "`'.")]
				$field: bool
			),+
		}

		impl $structure {
			/// A set made up of each flag identifier.
			const $lone: [char; 0 $( + { $flag /* i hate this */; 1 })+] = [$($flag),+];

			fn from_map(map: HashSet<char>) -> Self {
				Self { $($field: map.contains(&$flag)),+ }
			}

			$(
				#[doc = concat!("Refer to [`", stringify!($field), "`] for more information.")]
				///
				#[doc = concat!("[`", stringify!($field), "`]: Self#field.", stringify!($field))]
				// macro bullshit
				pub(crate) fn $field(&self) -> bool { self.$field }
			)+
		}
	};
}
use create_flags; // shitty workaround
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl Bundle {
	/// Convenience function for less repetition.
	///
	/// # Panics:
	///
	/// - An output device could not be determined. (refer to [`OutputStream's try_default`])
	///
	/// [`OutputStream's try_default`]: rodio::OutputStream::try_default
	fn output_device() -> (OutputStream, OutputStreamHandle) {
		log!(info: "Determining the output device.");
		rodio::OutputStream::try_default()
			.unwrap_or_else(|why|
				{
					if let Err(why) = disable_raw_mode() { log!(err: "disable the raw mode of the current terminal" => why) }
					panic!("determine the default audio output device  {why}")
				}
			)
	}

	/// Initialize.
	pub(crate) fn new() -> Self {
		let sound_out = Self::output_device();

		let (signal_sender, signal_receiver) = unbounded();
		let (exit_notifier, exit_receiver) = unbounded();
		let controls = 'controls: {
			let Ok(_) = event::poll(TICK) else { break 'controls None };

			log!(info: "Spinning up the control thread.");
			let Ok(control_thread) = Builder::new()
				.name(String::from("Control"))
				.spawn(move ||
					while let Err(RecvTimeoutError::Timeout) = exit_receiver.recv_timeout(TICK) {
						if !event::poll(TICK).unwrap_or_else(|why| panic!("poll an event from the current terminal  {why}")) { continue }
						let signal = match event::read().unwrap_or_else(|why| panic!("read an event from the current terminal  {why}")) {
							Event::Key(KeyEvent { code: KeyCode::Char(code), modifiers, .. }) => {
								#[cfg(debug_assertions)] print!("\r{code:?} {modifiers:?}\n\0");
								match code {
									'l' | 'L' if modifiers.contains(KeyModifiers::CONTROL) => Layer::Playlist(Control(Signal::Increment)),
									'j' | 'J' if modifiers.contains(KeyModifiers::CONTROL) => Layer::Playlist(Control(Signal::Decrement)),
									'k' | 'k' if modifiers.contains(KeyModifiers::CONTROL) => {
										if let Err(why) = signal_sender.send(Layer::Playlist(Control(Signal::Toggle))) { log!(err: "send a signal to the playback" => why) }
										return
									},

									'l' => Layer::Track(Other(Signal::Increment)),
									'j' => Layer::Track(Other(Signal::Decrement)),
									'k' => Layer::Track(Other(Signal::Toggle)),

									'L' => Layer::Volume(Shift(Signal::Increment)),
									'J' => Layer::Volume(Shift(Signal::Decrement)),
									'K' => Layer::Volume(Shift(Signal::Toggle)),

									_ => continue,
								}
							}
							#[allow(unused_variables)] event => {
								#[cfg(debug_assertions)] print!("\r{event:?}\n\0");
								continue
							}
						};
						#[cfg(debug_assertions)] print!("\r{signal:?}\n\0");
						if let Err(_) = signal_sender.send(signal) { panic!("send a signal to the playback  {DISCONNECTED}") }
					}
				)
				.map_err(|why| log!(err: "create the control thread" => why)) else { break 'controls None };

			Some(Controls { control_thread, exit_notifier, signal_receiver })
		};
		if controls.is_none() { log!(info: "Starting in headless mode.") }

		print!("\n\r\0");
		Self {
			sound_out,
			controls,
		}
	}

	/// Initialise without a head.
	pub(crate) fn headless() -> Self {
		Self {
			sound_out: Self::output_device(),
			controls: None,
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
	pub(crate) fn receive_signal(&self, moment: Instant) -> Result<Layer, RecvTimeoutError> {
		self
			.signal_receiver
			.recv_deadline(moment + TICK)
		// .inspect(|signal| { #[cfg(debug_assertions)] print!("\r{signal:?}\n\0") }) // commented out because unstable interface
	}
}

impl Flags {
	/// Split the program arguments into files and flags.
	///
	/// # Panics:
	///
	/// - Arguments are empty.
	pub(crate) fn new() -> (Self, impl Iterator<Item = String>) {
		let mut flag_count = 0;
		let flags = { // perform argument checks
			let mut arguments = args()
				.skip(1) // skips the executable path (e.g.: //bin/{bin-name})
				.peekable();
			if let None = arguments.peek() { panic!("get the program arguments  no arguments given") }
			arguments
		}
			.map_while(|argument|
				{
					let flag = argument.strip_prefix('-')?;
					flag_count += 1;
					flag
						.contains(Self::IDENTIFIERS)
						.then(|| String::from(flag))
				}
			)
			.collect::<String>();
		let mut flag_map = HashSet::with_capacity(flag_count);
		for key in flags.chars() { flag_map.insert(key); }

		(Self::from_map(flag_map), args().skip(flag_count + 1))
	}
}

impl Control {
	/// Manage the playlist's playback or program.
	pub(crate) fn manage(self, elapsed: Duration) -> Instruction {
		match self.0 {
			Signal::Increment => Instruction::Next,
			Signal::Decrement => if elapsed <= Duration::from_secs(1) { return Instruction::Back } else { return Instruction::Hold },
			Signal::Toggle => {
				clear();
				Instruction::Exit
			},
		}
	}
}

impl Other {
	/// Manage the track's playback.
	/// 
	/// # Values:
	/// - [`true`]: It signals that the track-loop should return a [`Hold`] [`Instruction`].
	/// - [`false`]: It signifies the exact opposite.
	///
	/// [`Hold`]: crate::songs::Instruction::Hold
	pub(crate) fn manage(self, playback: &Sink, elapsed: Duration, songs_index: &mut usize) -> bool {
		match self.0 {
			Signal::Increment => *songs_index += 1,
			Signal::Decrement => *songs_index -= (songs_index > &mut 0 && elapsed <= Duration::from_secs(1)) as usize,

			Signal::Toggle => {
				if playback.is_paused() { playback.play() } else { playback.pause() }
				return false
			},
		}
		true
	}
}

impl Shift {
	/// Manage the program's volume.
	pub(crate) fn manage(self, playback: &Sink, now: Instant, volume: &mut f32) -> Duration {
		match self.0 {
			Signal::Increment => *volume += 0.05,
			Signal::Decrement => *volume -= 0.05,
			Signal::Toggle => *volume += 2.0 * -*volume,
		}
		*volume = volume.clamp(-1.0, 2.0);
		playback.set_volume(volume.clamp(0.0, 2.0));
		if playback.is_paused() { return Duration::ZERO }
		now.elapsed()
	}
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
