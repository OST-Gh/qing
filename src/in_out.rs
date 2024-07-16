///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use crossbeam_channel::{self as channel, Receiver, Sender, TryRecvError};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
#[cfg(debug_assertions)]
use std::fmt::{self, Debug, Formatter};
use std::{
	io::{Read, Seek},
	thread::{Builder, JoinHandle},
};

use super::{ChannelError, Error};
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// This is a default message that is used when a [`Sender`] or [`Receiver`] has hung up the connection.
///
/// [`Sender`]: crossbeam_channel::Sender
/// [`Receiver`]: crossbeam_channel::Receiver
const DISCONNECTED: &str = "DISCONNECTED CHANNEL";
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Singleton bundled In- and Output constructs.
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
#[cfg_attr(
	any(debug_assertions, feature = "traits"),
	derive(PartialEq, Eq, PartialOrd, Ord),
	derive(Hash)
)]
#[repr(u8)]
/// High level control signal representation.
pub enum Signal {
	// 1 * 2^2 + 0 * 2^3
	PlaylistNext = 0b0101,  // 1 * 2^0 + 0 * 2^1
	PlaylistBack = 0b0110,  // 0 * 2^0 + 1 * 2^1
	Exit = 0b0111,          // 1 * 2^0 + 1 * 2^1
	PlaylistReset = 0b0100, // 0 * 2^0 + 0 * 2^1

	// 0 * 2^2 + 1 * 2^3
	TrackNext = 0b1001,  // 1 * 2^0 + 0 * 2^1
	TrackBack = 0b1010,  // 0 * 2^0 + 1 * 2^1
	Play = 0b1011,       // 1 * 2^0 + 1 * 2^1
	TrackReset = 0b1000, // 0 * 2^0 + 0 * 2^1

	// 1 * 2^2 + 1 * 2^3
	VolumeIncrease = 0b1101, // 1 * 2^0 + 0 * 2^1
	VolumeDecrease = 0b1110, // 0 * 2^0 + 1 * 2^1
	Mute = 0b1111,           // 1 * 2^0 + 1 * 2^1
	VolumeReset = 0b1100,    // 0 * 2^0 + 0 * 2^1
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl IOHandle {
	#[inline(always)]
	/// Get a reference to the underlying control structure.
	pub fn controls_get(&self) -> &Controls {
		&self.controls
	}

	#[inline(always)]
	/// Take the underlying [`Controls`].
	pub fn controls_take(self) -> Controls {
		self.controls
	}

	#[inline(always)]
	/// Get a reference to the [output-stream]
	///
	/// [output-stream]: OutputStreamHandle
	pub fn sound_out_handle_get(&self) -> &OutputStreamHandle {
		&self.sound_out
			.1
	}

	#[inline(always)]
	pub fn signal_receive(&self) -> Result<Signal, Error> {
		self.controls_get()
			.signal_receive()
			.map_err(ChannelError::from)
			.map_err(Error::Channel)
	}

	#[inline(always)]
	/// Get a reference to the underlying internal [`Sink`]
	///
	/// [`Sink`]: Sink
	pub fn playback_get(&self) -> &Sink {
		&self.playback
	}

	#[inline(always)]
	/// Play a single source.
	///
	/// A source is a read-, seek-able, synchronous source of bytes, that can be interpreted as a common file encoding.\
	/// See [`Decoder`]'s new associated functions.
	pub fn stream_play(
		&self,
		source: impl Read + Seek + Send + Sync + 'static,
	) -> Result<(), Error> {
		let decoder = Decoder::new(source)?;
		self.playback
			.append(decoder);
		Ok(())
	}

	/// Create a new [`IOHandle`] with an optional control-thread.
	pub fn try_new() -> Result<Self, Error> {
		let sound_out = rodio::OutputStream::try_default()?;

		let (signal_sender, signal_receiver) = channel::unbounded();
		let (exit_notifier, exit_receiver) = channel::unbounded();
		let key_handler = move || // NOTE(by: @OST-Gh): Pray to god that the caller actually joins the thread...
		loop {
			if !exit_receiver.is_empty() { return }
			let signal = match event::read().unwrap_or_else(|why| panic!("read an event from the current terminal  {why}")) {
				Event::Key(KeyEvent { code: KeyCode::Char('l' | 'L'), modifiers, .. }) if modifiers.contains(KeyModifiers::CONTROL) => Signal::PlaylistNext,
				Event::Key(KeyEvent { code: KeyCode::Char('j' | 'J'), modifiers, .. }) if modifiers.contains(KeyModifiers::CONTROL) => Signal::PlaylistBack,
				Event::Key(KeyEvent { code: KeyCode::Char('k' | 'K'), modifiers, .. }) if modifiers.contains(KeyModifiers::CONTROL) => Signal::Exit,
				Event::Key(KeyEvent { code: KeyCode::Char('h' | 'H'), modifiers, .. }) if modifiers.contains(KeyModifiers::CONTROL) => Signal::PlaylistReset,

				Event::Key(KeyEvent { code: KeyCode::Char('l'), ..}) => Signal::TrackNext,
				Event::Key(KeyEvent { code: KeyCode::Char('j'), ..}) => Signal::TrackBack,
				Event::Key(KeyEvent { code: KeyCode::Char('k'), ..}) => Signal::Play,
				Event::Key(KeyEvent { code: KeyCode::Char('h'), ..}) => Signal::TrackReset,

				Event::Key(KeyEvent { code: KeyCode::Char('L'), .. }) => Signal::VolumeIncrease,
				Event::Key(KeyEvent { code: KeyCode::Char('J'), .. }) => Signal::VolumeDecrease,
				Event::Key(KeyEvent { code: KeyCode::Char('K'), .. }) => Signal::Mute,
				Event::Key(KeyEvent { code: KeyCode::Char('H'), .. }) => Signal::VolumeReset,

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
		let controls = Controls {
			control_thread,
			exit_notifier,
			signal_receiver,
		};

		let playback = Sink::try_new(&sound_out.1)?;
		playback.pause();

		Ok(Self {
			sound_out,
			controls,
			playback,
		})
	}
}

#[cfg(any(debug_assertions, feature = "debug"))]
impl Debug for IOHandle {
	fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
		formatter
			.debug_struct("IOHandle")
			.field("controls", &self.controls)
			.finish_non_exhaustive()
	}
}

impl Controls {
	#[inline(always)]
	/// Utility function that calls [`exit_notify`] and [`clean_up`] in succession.
	///
	/// [`exit_notify`]: Self.exit_notify
	/// [`clean_up`]: Self.clean_up
	pub fn cleanly_exit(self) {
		self.exit_notify();
		self.clean_up()
	}

	#[inline(always)]
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

	#[inline(always)]
	/// Notify the control thread to exit if it hasn't already.
	///
	/// # Basic usage:
	///
	/// ```rust
	/// # use crate::in_out::IOHandle;
	/// let handle = IOHandle::try_new().unwrap;
	/// /* do stuff */
	///
	/// if let Some(control_reference) = handle.controls_get() { control_reference.notify_exit() }
	/// ```
	/// Used components: [`IOHandle`]'s [`controls_get`].
	///
	/// [`controls_get`]: IOHandle.controls_get
	pub fn exit_notify(&self) {
		let _ = self
			.exit_notifier
			.send(());
	}

	#[inline]
	/// Try to receive a signal, by waiting for it for a set amount of time.
	///
	/// This function is an analog to calling [`Receiver.try_recv`].
	///
	/// [`Receiver.try_recv`]: Receiver::try_recv
	pub fn signal_receive(&self) -> Result<Signal, TryRecvError> {
		self.signal_receiver
			.try_recv()
	}
}

macro_rules! pat {
	($this: expr => $($name: ident)|+) => {
		if let $(Self::$name)|+ = $this { true } else { false }
	}
}
impl Signal {
	#[inline(always)]
	/// Mask function that checks wether `self` is [`Next`] or [`Back`].
	///
	/// [`Next`]: Self.TrackNext
	/// [`Back`]: Self.TrackBack
	pub fn is_track_skip(&self) -> bool {
		pat!(self => TrackNext | TrackBack)
	}
	#[inline(always)]
	/// Mask function that checks wether `self` is [`Next`] or [`Back`].
	///
	/// [`Next`]: Self.PlaylistNext
	/// [`Back`]: Self.PlaylistBack
	pub fn is_playlist_skip(&self) -> bool {
		pat!(self => PlaylistNext | PlaylistBack)
	}
	#[inline(always)]
	/// Mask fucntion that checks if `self` is a [`Playlist`] or [`Track`] level `Next`
	///
	/// [`Playlist`]: Self.PlaylistNext
	/// [`Track`]: Self.TrackNext
	pub fn is_next_skip(&self) -> bool {
		pat!(self => TrackNext | PlaylistNext)
	}
	#[inline(always)]
	/// Mask fucntion that checks if `self` is a [`Playlist`] or [`Track`] level `Back`
	///
	/// [`Playlist`]: Self.PlaylistBack
	/// [`Track`]: Self.TrackBack
	pub fn is_back_skip(&self) -> bool {
		pat!(self => TrackBack | PlaylistBack)
	}
	#[inline(always)]
	/// Mask function that checks wether `self` is one o
	pub fn is_skip(&self) -> bool {
		pat!(self => TrackNext | TrackBack | PlaylistNext | PlaylistBack)
	}
	#[inline(always)]
	pub fn is_reset(&self) -> bool {
		pat!(self => PlaylistReset | TrackReset | VolumeReset)
	}

	#[inline(always)]
	pub fn is_playlist(&self) -> bool {
		pat!(self => PlaylistNext | PlaylistBack | PlaylistReset)
	}
	#[inline(always)]
	pub fn is_track(&self) -> bool {
		pat!(self => TrackNext | TrackBack | TrackReset)
	}
	#[inline(always)]
	pub fn is_volume(&self) -> bool {
		pat!(self => VolumeIncrease | VolumeDecrease | Mute | VolumeReset)
	}
}
