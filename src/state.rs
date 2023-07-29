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
use std::thread::{ Builder, JoinHandle };
use crossterm::event::{
	self,
	Event,
	KeyEvent,
	KeyCode,
};
use std::{
	fs::File,
	io::BufReader,
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
	output: (OutputStream, OutputStreamHandle),
	control: JoinHandle<()>,
	exit: Sender<u8>,
	signal: Receiver<Signal>,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// High level control signal representation
pub(crate) enum Signal {
	ProgramExit,
	PlaylistNext,
	PlaylistBack,
	SongNext,
	SongBack,
	PlaybackToggle,
	VolumeToggle,
	VolumeIncrease,
	VolumeDecrease,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl State {
	/// Initialize state.
	pub(crate) fn initialise() -> Self {
		log!(info: "Determining the output device.");
		let output = rodio::OutputStream::try_default()
			.unwrap_or_else(|why|
				{
					if let Err(why) = disable_raw_mode() { log!(err: "disable the raw mode of the current terminal" => why) }
					panic!("determine the default audio output device  {why}")
				}
			);


		log!(info: "Spinning up the playback control thread.");
		let (sender, signal) = unbounded();
		let (exit, exit_receiver) = unbounded();
		let control = Builder::new()
			.name(String::from("Playback-Control"))
			.spawn(move ||
				while let Err(RecvTimeoutError::Timeout) = exit_receiver.recv_timeout(TICK) {
					if !event::poll(TICK).unwrap_or_else(|why| panic!("poll an event from the current terminal  {why}")) { continue }
					let signal = match event::read().unwrap_or_else(|why| panic!("read an event from the current terminal  {why}")) {
						Event::Key(KeyEvent { code: KeyCode::Char('c' | 'C'), .. }) => {
							if let Err(why) = sender.send(Signal::ProgramExit) { log!(err: "send a signal to the playback" => why) }
							return
						},
						Event::Key(KeyEvent { code: KeyCode::Char('n' | 'N'), .. }) => Signal::PlaylistNext,
						Event::Key(KeyEvent { code: KeyCode::Char('b' | 'B'), .. }) => Signal::PlaylistBack,
						Event::Key(KeyEvent { code: KeyCode::Char('l' | 'L'), .. }) => Signal::SongNext,
						Event::Key(KeyEvent { code: KeyCode::Char('j' | 'J'), .. }) => Signal::SongBack,
						Event::Key(KeyEvent { code: KeyCode::Char('k' | 'K'), .. }) => Signal::PlaybackToggle,
						Event::Key(KeyEvent { code: KeyCode::Char('m' | 'M'), .. }) => Signal::VolumeToggle,
						Event::Key(KeyEvent { code: KeyCode::Up             , .. }) => Signal::VolumeIncrease,
						Event::Key(KeyEvent { code: KeyCode::Down           , .. }) => Signal::VolumeDecrease,
						_ => continue,
					};
					if let Err(_) = sender.send(signal) { panic!("send a signal to the playback  {DISCONNECTED}") }
				}
			)
			.unwrap_or_else(|why| panic!("create the playback control thread  {why}"));

		Self {
			output,
			control,
			exit,
			signal,
		}
	}

	/// Clean-up the state.
	pub(crate) fn clean_up(self) {
		if let Err(why) = self
			.control
			.join()
		{
			let why = why
				.downcast_ref::<String>()
				.unwrap();
			log!(err: "clean up the playback control thread" => why)
		}
	}

	/// Notify the playback control thread to exit if it hasn't already.
	pub(crate) fn notify_exit(&self) {
		let _ = self
			.exit
			.send(0);
	}

	/// Check wether or not the playback control thread is still running.
	pub(crate) fn is_alive(&self) -> bool {
		!self
			.control
			.is_finished()
	}

	/// Play a single file.
	pub(crate) fn play_file(&self, song: &'static mut BufReader<File>) -> Result<Sink, PlayError> {
		self
			.output
			.1
			.play_once(song)
	}

	/// Try to receive a signal by waiting for it for a set amount of time.
	pub(crate) fn receive_signal(&self, moment: Instant) -> Result<Signal, RecvTimeoutError> {
		self
			.signal
			.recv_deadline(moment + TICK)
	}
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
