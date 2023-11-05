///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use rodio::{
	OutputStream,
	OutputStreamHandle,
	Decoder,
	Sink,
};
use crossbeam_channel::{
	self as channel,
	Sender,
	Receiver,
	RecvTimeoutError,
};
use crossterm::event::{
	self,
	Event,
	KeyEvent,
	KeyCode,
	KeyModifiers,
};
use std::{
	time::Instant,
	thread::{ self, JoinHandle },
	io::{
		Seek,
		Read,
	},
};
#[cfg(debug_assertions)] use std::fmt::{
	self,
	Formatter,
	Debug,
};
use super::TICK;
use super::Error;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// This is a default message that is used when a [`Sender`] or [`Receiver`] has hung up the connection.
///
/// [`Sender`]: crossbeam_channel::Sender
/// [`Receiver`]: crossbeam_channel::Receiver
const DISCONNECTED: &str = "DISCONNECTED CHANNEL";
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
pub struct IOHandle {
	sound_out: (OutputStream, OutputStreamHandle), // NOTE(by: @OST-Gh): Needs to be tuple, otherwise breaks
	controls: Option<Controls>,
	playback: Sink,
}

#[cfg_attr(debug_assertions, derive(Debug))]
/// A wrapper around a thread handle.
///
/// This structure bundles: The control thread handle, a sender, and a receiver.\
/// The sender's purpose is to notify the control thread that it should exit.\
/// On the other hand, the receiver exists in order to receive [`signals`] from the control thread.\
/// Said control thread is responsible for reading keyboard inputs from a, raw mode set, terminal, and parsing them into [`signals`].
///
/// [`signals`]: Signal
pub struct Controls {
	control_thread: JoinHandle<()>,
	exit_notifier: Sender<()>,
	signal_receiver: Receiver<Signal>,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(debug_assertions, derive(Debug))]
#[repr(u8)]
/// High level control signal representation.
pub enum Signal {
	PlaylistNext	= 0b000001_01,
	PlaylistBack	= 0b000001_10,
	Exit		= 0b000001_11,

	TrackNext	= 0b000010_01,
	TrackBack	= 0b000010_10,
	Play		= 0b000010_11,

	VolumeIncrease	= 0b000011_01,
	VolumeDecrease	= 0b000011_10,
	Mute		= 0b000011_11,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl IOHandle {
	/// Get a reference to the underlying control structure.
	pub fn controls_get(&self) -> Option<&Controls> {
		self
			.controls
			.as_ref()
	}

	pub fn sound_out_handle_get(&self) -> &OutputStreamHandle {
		&self
			.sound_out
			.1
	}

	/// Take the underlying controls.
	pub fn controls_take(self) -> Option<Controls> { self.controls }

	#[inline]
	pub fn playback_get(&self) -> &Sink { &self.playback }

	/// Play a single source.
	pub fn stream_play(&self, source: impl Read + Seek + Send + Sync + 'static) -> Result<(), Error> {
		let decoder = Decoder::new(source)?;
		self
			.playback
			.append(decoder);
		Ok(())
	}
}

impl TryFrom<bool> for IOHandle {
	type Error = Error;

	/// Create a new [`Bundle`] with an optional control-thread.
	fn try_from(is_headless: bool) -> Result<Self, Error> {
		let sound_out = rodio::OutputStream::try_default()?;

		let controls = is_headless.then(||
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
								Event::Key(KeyEvent { code: KeyCode::Char('k' | 'K'), modifiers, .. }) if modifiers.contains(KeyModifiers::CONTROL) => return if let Err(why) = signal_sender.send(Signal::Exit) { panic!("sending a signal  {why}") },

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

		let playback = Sink::try_new(&sound_out.1)?;
		playback.pause();

		Ok(
			Self {
				sound_out,
				controls,
				playback,
			}
		)
	}
}

#[cfg(debug_assertions)]
impl Debug for IOHandle {
	fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
		formatter
			.debug_struct("IOHandle")
			.field("controls", &self.controls)
			.finish_non_exhaustive()
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
	pub fn clean_up(self) {
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
	pub fn exit_notify(&self) {
		let _ = self
			.exit_notifier
			.send(());
	}

	/// Try to receive a signal, by waiting for it for a set amount of time.
	pub fn signal_receive(&self, moment: Instant) -> Result<Signal, RecvTimeoutError> {
		self
			.signal_receiver
			.recv_deadline(moment + TICK)
	}
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
// 		}
// 		*volume = volume.clamp(-1.0, 2.0);
// 		playback.set_volume(volume.clamp(0.0, 2.0));
// 		if playback.is_paused() { return Duration::ZERO }
// 		now.elapsed()
// 	}
// }
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
