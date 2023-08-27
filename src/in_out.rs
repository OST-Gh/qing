///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use rodio::{
	OutputStream,
	OutputStreamHandle,
	Sink,
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
use super::{
	TICK,
	DISCONNECTED,
	Instant,
	RecvTimeoutError,
	log,
	disable_raw_mode,
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
	sound_out: (OutputStream, OutputStreamHandle),
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
/// [`program arguments`]: args
pub(crate) struct Flags {
	/// If wether, or not, the program should merge all, passed in, lists into one.
	///
	/// Specify using: -f
	pub flatten: bool,
	/// If quing should spawn a control-thread, or not.
	///
	/// Specify using: -h
	pub headless: bool,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(debug_assertions, derive(Debug))]
/// High level control signal representation.
pub(crate) enum Signal {
	PlaylistNext,
	PlaylistBack,
	ProgramExit,

	TrackNext,
	TrackBack,
	PlaybackToggle,

	VolumeIncrease,
	VolumeDecrease,
	VolumeToggle,
}
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
							Event::Key(KeyEvent { code: KeyCode::Char(code), modifiers, .. }) => match code {
								'l' | 'L' if modifiers.contains(KeyModifiers::CONTROL) => Signal::PlaylistNext,
								'j' | 'J' if modifiers.contains(KeyModifiers::CONTROL) => Signal::PlaylistBack,
								'k' | 'k' if modifiers.contains(KeyModifiers::CONTROL) => {
									if let Err(why) = signal_sender.send(Signal::ProgramExit) { log!(err: "send a signal to the playback" => why) }
									return
								},

								'l' => Signal::TrackNext,
								'j' => Signal::TrackBack,
								'k' => Signal::PlaybackToggle,

								'L' => Signal::VolumeIncrease,
								'J' => Signal::VolumeDecrease,
								'K' => Signal::VolumeToggle,

								_ => continue,
							}
							#[allow(unused_variables)] event => {
								#[cfg(debug_assertions)] print!("\r{event:?}\0\n");
								continue
							}
						};
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
	pub(crate) fn receive_signal(&self, moment: Instant) -> Result<Signal, RecvTimeoutError> {
		self
			.signal_receiver
			.recv_deadline(moment + TICK)
		// .inspect(|signal| { #[cfg(debug_assertions)] print!("\r{signal:?}\0\n") }) // commented out because unstable interface
	}
}

impl Flags {
	/// Split the program arguments into files and flags.
	///
	/// # Panics:
	///
	/// - Arguments are empty.
	pub(crate) fn new() -> (Self, impl Iterator<Item = String>) {
		macro_rules! create_flag_identifiers {
			($($name: ident = $flag: literal)+ [$lone: ident]) => {
				$(const $name: char = $flag;)+
				const $lone: &[char] = &[$($name),+];
			}
		}
		create_flag_identifiers!(
			HEADLESS = 'h'
			FLATTEN = 'f'
			[IDENTIFIERS]
		);

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
						.contains(IDENTIFIERS)
						.then(|| String::from(flag))
				}
			)
			.collect::<String>();
		let mut flag_map = HashSet::with_capacity(flag_count);
		for key in flags.chars() { flag_map.insert(key); }

		(
			Self {
				flatten: flag_map.contains(&FLATTEN),
				headless: flag_map.contains(&HEADLESS),
			},
			args().skip(flag_count + 1),
		)
	}
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
