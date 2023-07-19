///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	thread::{ spawn, JoinHandle },
	fs::{ self, File },
	path::{ PathBuf, MAIN_SEPARATOR_STR },
	time::{ Duration, Instant },
	io::{ BufReader, Seek },
	env::{ var, args },
	cell::OnceCell,
	panic,
	any::Any,
};
use crossterm::{
	terminal::{ enable_raw_mode, disable_raw_mode },
	event::{
		self,
		Event,
		KeyEvent,
		KeyCode,
	},
};
use lofty::{ read_from_path, AudioFile };
use crossbeam_channel::{ unbounded, RecvTimeoutError, Sender, Receiver };
use rodio::{ OutputStream, OutputStreamHandle };
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
const FOURTH_SECOND: Duration = Duration::from_millis(250);
const SECOND: Duration = Duration::from_secs(1);

const NO_DISPLAY: &'static str = "NO_DISPLAYABLE_ERROR_INFORMATION";
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
static mut FILES: Vec<BufReader<File>> = Vec::new();
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[derive(serde::Deserialize)]
struct Songlist {
	name: Box<str>,
	song: Vec<Song>,
}

#[derive(serde::Deserialize)]
struct Song {
	name: Box<str>,
	file: Box<str>,
}

struct State {
	output: (OutputStream, OutputStreamHandle),
	control: JoinHandle<()>,
	exit: Sender<u8>,
	signal: Receiver<Signal>,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
enum Signal {
	ManualExit, // signal sent by pb_ctl to main for indication of the user manually exiting
	SkipPlaylist,
	SkipNext,
	SkipBack,
	TogglePlayback,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
macro_rules! log {
	(err$([$($visible: ident)+])?: $message: literal => $($why: ident)+ $(; $($retaliation: tt)+)?) => {
		{
			print!(concat!("\r\x1b[38;2;254;205;33m\x1b[4mAn error occured whilst attempting to ", $message, ';') $(, $($visible = $visible),+)?);
			print!(" '\x1b[1m");
			$(print!("\n\r{}", $why);)+
			print!("\x1b[22m'");
			println!("\x1b[24m\0");
			$($($retaliation)+)?
		}
	};
	(info$([$($visible: ident)+])?: $message: literal) => { println!(concat!("\r\x1b[38;2;254;205;33m", $message, '\0') $(, $($visible = $visible),+)?) };
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
fn fmt_path(text: impl AsRef<str>) -> PathBuf {
	PathBuf::from(
		text
			.as_ref()
			.split(MAIN_SEPARATOR_STR)
			.filter_map(|part|
				match match part {
					"~" => var("HOME"),
					_ if part.starts_with('$') => var(&part[1..]),
					_ => return Some(String::from(part)),
				} {
					Ok(part) => Some(part),
					Err(why) => log!(err: "expand a shell expression to a path" => why; None)
				}
			)
			.collect::<Vec<String>>()
			.join(MAIN_SEPARATOR_STR)
	)
		.canonicalize()
		.unwrap_or_else(|why| log!(err: "canonicalise a path" => why; PathBuf::new()))
}

fn panic_payload(payload: &(dyn Any + Send)) -> String {
	(&payload)
		.downcast_ref::<&str>()
		.map(|slice| String::from(*slice))
		.xor(
			payload
				.downcast_ref::<String>()
				.map(String::from)
		)
		.unwrap()
}

fn panic_handle(info: &panic::PanicInfo) {
	let panic = panic_payload(info.payload());
	unsafe {
		let panic = panic
			.splitn(2, "  ")
			.collect::<Vec<&str>>();
		let message = panic.get_unchecked(0);
		let reason = panic
			.get(1)
			.unwrap_or(&"NO_DISPLAYABLE_INFO")
			.replace('\n', "\r\n");
		println!("\r\x1b[38;2;254;205;33m\x1b[4mAn error occured whilst attempting to {message}; '\x1b[1m{reason}\x1b[22m'\x1b[24m\0")
	};
}

fn main() {
	panic::set_hook(Box::new(panic_handle));

	let mut files = args()
		.skip(1) // skips the executable path (e.g.: //usr/local/bin/{bin-name})
		.peekable();
	if files
		.peek()
		.is_none() // can't run without one playlist
	{ panic!("get the program arguments  no arguments given") }

	if let Err(why) = enable_raw_mode() { log!(err: "enable the raw mode of the current terminal" => why; return) }

	let mut generator = fastrand::Rng::new();
	let init = OnceCell::new(); // expensive operation only executed if no err.

	'queue: for path in files {

		log!(info[path]: "Loading and parsing data from [{path}].");
		let Songlist { mut song, name } = match fs::read_to_string(fmt_path(&path)).map(|contents| toml::from_str(&contents)) {
			Ok(Ok(playlist)) => playlist,
			Ok(Err(why)) => log!(err[path]: "parse the contents of [{path}]" => why; continue 'queue),
			Err(why) => log!(err[path]: "load the contents of [{path}]" => why; continue 'queue),
		};

		let song: Vec<(Box<str>, Duration)> = {

			log!(info[name]: "Shuffling all of the songs in [{name}].");
			let length = song.len();
			for _ in 0..length * length { // l^2 for more shuffling and less chance for order
				let old = generator.usize(0..length);
				let new = generator.usize(0..length);
				song.swap(old, new)
			}

			log!(info[name]: "Loading all of the audio contents of the songs in [{name}].");
			song
				.into_iter()
				.filter_map(|Song { name, file }|
					{
						let formatted = fmt_path(file);
						match (File::open(&formatted), read_from_path(formatted)) {
							(Ok(contents), Ok(info)) => {
								unsafe { FILES.push(BufReader::new(contents)) }
								return Some(info.properties()).map(|info| (name, info.duration()))
							},
							(Err(why), Ok(_)) => log!(err[name]: "load the audio contents of [{name}]" => why),
							(Ok(_), Err(why)) => log!(err[name]: "load the audio properties of [{name}]" => why),
							(Err(file_why), Err(info_why)) => log!(err[name]: "load the audio contents and properties of [{name}]" => file_why info_why),
						}
						None
					}
				)
				.collect()
		};
		println!("\r\0");

		let State { output, control, signal, .. } = init.get_or_init(State::initialise);


		let length = song.len();
		let mut index = 0;

		'playlist: while index < length && !control.is_finished() {
			let old = index; // (sort of) proxy to index (used because of rewind code)
			// unless something is very wrong with the index (old), this will not error.
			let (name, duration) = unsafe { song.get_unchecked(old) };
			match output
				.1
				.play_once(unsafe { FILES.get_unchecked_mut(old) })
			{
				Ok(playback) => 'playback: {
					log!(info[name]: "Playing back the audio contents of [{name}].");
					let mut elapsed = Duration::ZERO;
					while &elapsed <= duration {
						let now = Instant::now();
						let paused = playback.is_paused();
						elapsed += match signal.recv_deadline(now + FOURTH_SECOND) {
							Err(RecvTimeoutError::Timeout) => if paused { continue } else { FOURTH_SECOND },

							Ok(Signal::ManualExit) => break 'queue,
							Ok(Signal::SkipPlaylist) => break 'playlist,
							Ok(Signal::SkipNext) => break,
							Ok(Signal::SkipBack) => break 'playback index -= (old > 0 && elapsed <= SECOND) as usize,
							Ok(Signal::TogglePlayback) => {
								if paused { playback.play() } else { playback.pause() }
								now.elapsed()
							},

							Err(RecvTimeoutError::Disconnected) => break 'queue, // chain reaction will follow
						};
					}
					index += 1
				},
				Err(why) => log!(err[name]: "playback [{name}] from the default audio output device" => why; break 'queue), // assume error will occur on the other tracks too
			};
			if let Err(why) = unsafe { FILES.get_unchecked_mut(old) }.rewind() { log!(err[name]: "reset the player position inside of [{name}]" => why) }
		}
		unsafe { FILES.clear() }
	}

	init
		.into_inner()
		.map(State::clean_up);
	if let Err(why) = disable_raw_mode() { log!(err: "disable the raw mode of the current terminal" => why) }
	print!("\r\x1b[0m\0")
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl State {
	fn initialise() -> Self {
		log!(info: "Determining the output device.");
		let output = match rodio::OutputStream::try_default() {
			Ok(handles) => handles,
			Err(why) => {
				if let Err(why) = disable_raw_mode() { log!(err: "disable the raw mode of the current terminal" => why) }
				panic!("determine the default audio output device  {why}")
			},
		};


		log!(info: "Spinning up the playback control thread.");
		let (sender, signal) = unbounded();
		let (exit, exit_receiver) = unbounded();
		let control = spawn(move ||
			while let Err(RecvTimeoutError::Timeout) = exit_receiver.recv_timeout(FOURTH_SECOND) {
				let signal = match match event::poll(FOURTH_SECOND) {
					Ok(truth) => if truth { event::read() } else { continue },
					Err(why) => panic!("poll an event from the current terminal  {why}"),
				} {
					Ok(Event::Key(KeyEvent { code: KeyCode::Char('c' | 'C'), .. })) => {
						if let Err(why) = sender.send(Signal::ManualExit) { log!(err: "send a signal to the playback" => why) }
						break
					},
					Ok(Event::Key(KeyEvent { code: KeyCode::Char('n' | 'N'), .. })) => Signal::SkipPlaylist,
					Ok(Event::Key(KeyEvent { code: KeyCode::Char('l' | 'L'), .. })) => Signal::SkipNext,
					Ok(Event::Key(KeyEvent { code: KeyCode::Char('j' | 'J'), .. })) => Signal::SkipBack,
					Ok(Event::Key(KeyEvent { code: KeyCode::Char('k' | 'K'), .. })) => Signal::TogglePlayback,
					Err(why) => panic!("read an event from the current terminal  {why}"),
					_ => continue,
				};
				if let Err(_) = sender.send(signal) { panic!("send a signal to the playback") }
			}
		);

		Self {
			output,
			control,
			exit,
			signal,
		}
	}

	fn clean_up(self) {
		let Self { control, exit, .. } = self;

		if !control.is_finished() {
			if let Err(_) = exit.send(0) { log!(err: "send the exit signal to the playback control thread" => NO_DISPLAY) }
		} // assume that there's no error here. if there is, then the thread either finished(/panicked) 
		if let Err(why) = control.join() {
			let why = panic_payload(&why);
			log!(err: "clean up the playback control thread" => why)
		}
	}
}

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////