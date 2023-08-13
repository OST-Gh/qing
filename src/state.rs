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
	fs::File,
	io::BufReader,
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
/// Bundled lazily initialised values.
///
/// The values, that the structure contains, will be initialised if the program successfully loads at least a single playlist.
/// Generally, this means, that this state type is always contained inside a type that can be uninitialised, e.g: OnceCell, or a mutable Option.
pub(crate) struct State {
	sound_out: (OutputStream, OutputStreamHandle),
	controls: Option<Controls>,
}

/// Controls
pub(crate) struct Controls {
	control_thread: JoinHandle<()>,
	exit_notifier: Sender<()>,
	signal_receiver: Receiver<Signal>,
}

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// High level control signal representation
#[cfg_attr(debug_assertions, derive(Debug))]
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
impl State {
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

	/// Initialize state.
	pub(crate) fn new() -> Self {
		let sound_out = Self::output_device();

		let (signal_sender, signal_receiver) = unbounded();
		let (exit_notifier, exit_receiver) = unbounded();
		let controls = 'controls: {
			let Ok(_) = event::poll(TICK) else { break 'controls None };

			log!(info: "Spinning up the playback control thread.");
			let Ok(control_thread) = Builder::new()
				.name(String::from("Playback-Control"))
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
				.map_err(|why| log!(err: "create the playback control thread" => why)) else { break 'controls None };

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
	/// Clean-up the state.
	pub(crate) fn clean_up(self) {
		let _ = self
			.control_thread
			.join();
	}


	/// Notify the playback control thread to exit if it hasn't already.
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
		// .inspect(|signal| { #[cfg(debug_assertions)] print!("\r{signal:?}\0\n") }) // commented out because unstable interface
	}
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
