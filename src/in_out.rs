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
	thread::{ Builder, JoinHandle },
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

use super::{
	TICK,
	Error,
	ChannelError,
};
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// This is a default message that is used when a [`Sender`] or [`Receiver`] has hung up the connection.
///
/// [`Sender`]: crossbeam_channel::Sender
/// [`Receiver`]: crossbeam_channel::Receiver
const DISCONNECTED: &str = "DISCONNECTED CHANNEL";
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Bundled In- and Output constructs.
///
/// # Basic usage
///
/// ```rust
/// # use crate::in_out::IOHandle;
/// let handle = IOHandle::try_new().unwrap();
/// /* do stuff */
/// ```
pub struct IOHandle {
	sound_out: (OutputStream, OutputStreamHandle), // NOTE(by: @OST-Gh): Needs to be tuple, otherwise breaks
	controls: Controls,
	playback: Sink,
}

#[cfg_attr(any(debug_assertions, feature = "debug"), derive(Debug))]
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
#[cfg_attr(any(debug_assertions, feature = "debug"), derive(Debug))]
#[cfg_attr(any(debug_assertions, feature = "traits"), derive(PartialEq, Eq, PartialOrd, Ord), derive(Hash))]
#[repr(u8)]
/// High level control signal representation.
pub enum Signal {
	PlaylistNext	= 0b0000_0101,
	PlaylistBack	= 0b0000_0110,
	Exit		= 0b0000_0111,

	TrackNext	= 0b0000_1001,
	TrackBack	= 0b0000_1010,
	Play		= 0b0000_1011,

	VolumeIncrease	= 0b0000_1101,
	VolumeDecrease	= 0b0000_1110,
	Mute		= 0b0000_1111,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl IOHandle {
	#[inline]
	/// Get a reference to the underlying control structure.
	pub fn controls_get(&self) -> &Controls { &self.controls }

	#[inline]
	/// Take the underlying [`Controls`].
	pub fn controls_take(self) -> Controls { self.controls }

	/// Get a reference to the [output-stream]
	///
	/// [output-stream]: OutputStreamHandle
	pub fn sound_out_handle_get(&self) -> &OutputStreamHandle {
		&self
			.sound_out
			.1
	}

	#[inline(always)]
	pub fn signal_receive(&self, moment: Instant) -> Result<Signal, Error> {
		self
			.controls_get()
			.signal_receive(moment)
			.map_err(ChannelError::from)
			.map_err(Error::Channel)
	}

	#[inline]
	/// Get a reference to the underlying internal [`Sink`]
	///
	/// [`Sink`]: Sink
	pub fn playback_get(&self) -> &Sink { &self.playback }

	#[inline]
	/// Play a single source.
	///
	/// A source is a read-, seekable, syncronous source of bytes, that can be interpreted as a common file encoding.\
	/// See [`Decoder`]'s new associated functions.
	pub fn stream_play(&self, source: impl Read + Seek + Send + Sync + 'static) -> Result<(), Error> {
		let decoder = Decoder::new(source)?;
		self
			.playback
			.append(decoder);
		Ok(())
	}

	/// Create a new [`IOHandle`] with an optional control-thread.
	pub fn try_new() -> Result<Self, Error> {
		let sound_out = rodio::OutputStream::try_default()?;

		let controls = {
			let (signal_sender, signal_receiver) = channel::unbounded();
			let (exit_notifier, exit_receiver) = channel::unbounded();
			let key_handler = move ||
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
				if signal_sender
					.send(signal)
					.is_err()
				{ panic!("send a signal to the playback  {DISCONNECTED}") }
			};
			let control_thread = Builder::new()
				.name(String::from("Controls"))
				.stack_size(8)
				.spawn(key_handler)?;
			Controls {
				control_thread,
				exit_notifier,
				signal_receiver,
			}
		};

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
	/// Supposed to be used in conjunction with [`exit_notify`].
	///
	/// # Basic usage:
	///
	/// ```rust
	/// # use crate::in_out::IOHandle;
	/// let handle = IOHandle::new();
	/// /* do stuff */
	///
	/// let controls = handle.take_controls();
	/// controls.notify_exit();
	/// controls.clean_up()
	/// ```
	/// Used things: [`exit_notify`], [`IOHandle`], and [`controls_take`].
	///
	/// [`exit_notify`]: Self.exit_notify
	/// [`controls_take`]: IOHandle.controls_take
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
	/// # use crate::in_out::IOHandle;
	/// let handle = IOHandle::try_new().unwrap;
	/// /* do stuff */
	///
	/// if let Some(control_reference) = handle.controls_get() { control_refernce.notify_exit() }
	/// ```
	/// Used components: [`IOHandle`]'s [`controls_get`].
	///
	/// [`controls_get`]: IOHandle.controls_get
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

macro_rules! pat {
	($this: expr => $($name: ident)|+) => {
		if let $(Self::$name)|+ = $this { true } else { false }
	}
}
impl Signal {
	#[inline(always)]
	pub fn track_skip_is(&self) -> bool { pat!(self => TrackNext | TrackBack) }

	#[inline(always)]
	pub fn playlist_skip_is(&self) -> bool { pat!(self => PlaylistNext | PlaylistBack) }

	#[inline(always)]
	pub fn next_skip_is(&self) -> bool { pat!(self => TrackNext | PlaylistNext) }

	#[inline(always)]
	pub fn back_skip_is(&self) -> bool { pat!(self => TrackBack | PlaylistBack) }

	#[inline(always)]
	pub fn skip_is(&self) -> bool { pat!(self => TrackNext | TrackBack | PlaylistNext | PlaylistBack) }


	#[inline(always)]
	pub fn volume_is(&self) -> bool { pat!(self => VolumeIncrease | VolumeDecrease | Mute) }
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
