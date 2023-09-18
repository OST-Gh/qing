///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
//! [I hate myself, for making documentation.]
//!
//! ### How Quing works.
//! Quing works around 2 central structures:
//! - A [`Track`]
//! - A [`Playlist`] (grouping of [`Tracks`], with additional data)
//!
//! [`Track`]: songs::Track
//! [`Tracks`]: songs::Track
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	panic,
	cell::OnceCell,
	env::args,
	path::{ MAIN_SEPARATOR_STR, PathBuf },
	time::{ Duration, Instant },
	env::{ VarError, var },
	io::{ stdout, stdin, BufRead },
};
use crossterm::{
	cursor::Hide,
	execute,
	tty::IsTty,
	terminal::{ enable_raw_mode, disable_raw_mode },
	style::{
		SetForegroundColor,
		Color,
	},
};
use crossbeam_channel::RecvTimeoutError;
use rodio::Sink;
use in_out::{ Bundle, Flags };
use echo::{ exit, clear };
use songs::Playlist;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// A module for handling and interacting with external devices.
mod in_out;

/// A collection of functions that are used repeatedly to display certain sequences.
mod echo;

/// A collection of file related structures, or implementations.
mod songs;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Constant signal [`Duration`] (tick rate). [250 milliseconds]
///
/// Every time related operation is tackted after this constant.\
const TICK: Duration = Duration::from_millis(250);
/// This is a default message that is used when a [`Sender`] or [`Receiver`] has hung up the connection.
///
/// [`Sender`]: crossbeam_channel::Sender
/// [`Receiver`]: crossbeam_channel::Receiver
const DISCONNECTED: &'static str = "DISCONNECTED CHANNEL";
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[macro_export]
/// A macro for general interaction with Standard-Out.
///
/// This macro is, in a general sense, just a fancier [`println`] macro, which also is more tailored towards [raw-mode].
///
/// [raw-mode]: crossterm::terminal#raw-mode
macro_rules! log {
	(err$([$($visible: ident)+])?: $message: literal => $($why: ident)+ $(; $($retaliation: tt)+)?) => {
		{
			print!(concat!("\rError whilst ", $message, ';') $(, $($visible = $visible),+)?);
			$(print!(" '{}'", format!("{}", $why).replace('\n', "\r\n"));)+
			print!("\n\0");
			$($($retaliation)+)?
		}
	};
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Format a text representation of a path into an absolute path.
///
/// This recursive function is used for unexpanded shell(zsh based) expressions, on a call site, and songs' file fields.
/// It can currently only expand environment variables, which might recurs.
fn fmt_path(path: impl AsRef<str>) -> PathBuf {
	fn expand(name: &str) -> Result<String, VarError> {
		let mut buffer = Vec::new();
		for part in var(if name.starts_with('$') { expand(&name[1..])? } else { String::from(name) })?
			.split(MAIN_SEPARATOR_STR)
			.map(|part| if part.starts_with('$') { expand(&part[1..]) } else { Ok(String::from(part)) })
		{ buffer.push(part?) }
		Ok(buffer.join(MAIN_SEPARATOR_STR))
	}

	let path = path.as_ref();
	PathBuf::from(
		path
			.split(MAIN_SEPARATOR_STR)
			.enumerate()
			.filter_map(|(index, part)|
				match match part {
					"~" if index == 0 => expand("HOME"),
					_ if part.starts_with('$') => expand(&part[1..]),
					_ => return Some(String::from(part)),
				} {
					Ok(part) => Some(part),
					Err(why) => log!(err[part]: "expanding [{part}] to a path" => why; None)
				}
			)
			.collect::<Vec<String>>()
			.join(MAIN_SEPARATOR_STR)
	)
		.canonicalize()
		.unwrap_or_else(|why| log!(err[path]: "canonicalising [{path}]" => why; PathBuf::new()))
}

fn main() {
	panic::set_hook(
		Box::new(|info|
			unsafe {
				let payload = info.payload();
				let panic = payload
					.downcast_ref::<&str>()
					.map(|slice| String::from(*slice))
					.xor(
						payload
							.downcast_ref::<String>()
							.map(String::from)
					)
					.unwrap();
				let panic = panic
					.splitn(2, "  ")
					.collect::<Vec<&str>>();
				let message = panic.get_unchecked(0);
				let reason = panic
					.get(1)
					.unwrap_or(&"NO_DISPLAYABLE_INFORMATION")
					.replace('\n', "\r\n");
				print!("\rAn error occurred whilst attempting to {message}; '{reason}'\n\0");
			exit();
			}
		)
	);

	let is_tty = stdin().is_tty();
	let mut arguments: Vec<String> = args()
		.skip(1) // skips the executable path (e.g.: //bin/{bin-name})
		.collect();
	if !is_tty {
		arguments.reserve(16);
		arguments.extend(
			stdin()
				.lock()
				.lines()
				.filter_map(Result::ok)
				.map(String::from)
		)
	};
	if let None = arguments.first() { panic!("get the program arguments  no arguments given") }
	let (flags, files) = Flags::separate_from(arguments);
	if !flags.should_spawn_headless() && is_tty {
		if let Err(why) = enable_raw_mode() { panic!("enable the raw mode of the current terminal  {why}") }
		if let Err(why) = execute!(stdout(),
			Hide,
			SetForegroundColor(Color::Yellow),
		) { log!(err: "setting the terminal style" => why) }
	}

	if flags.should_print_version() { print!(concat!('\r', env!("CARGO_PKG_NAME"), " on version ", env!("CARGO_PKG_VERSION"), " by ", env!("CARGO_PKG_AUTHORS"), ".\n\0")) }

	let (outlier, rest) = Playlist::from_outliers(files);
	let mut lists: Vec<Playlist> = rest
		.into_iter()
		.filter_map(|(contents, path)| Playlist::try_from_contents((contents, path)))
		.collect();
	lists.push(outlier);

	if flags.should_flatten() { lists = vec![Playlist::flatten(lists)] }

	let initialisable_bundle = OnceCell::new(); // expensive operation only executed if no err.

	let mut volume = 1.0;
	// 1 + 2 * -1 = 1 - 2 = -1 
	// -1 + 2 * 1 = -1 + 2 = 1

	let lists_length = lists.len();
	let mut lists_index = 0;
	while lists_index < lists_length {
		let old_lists_index = lists_index;
		let list = unsafe { lists.get_unchecked_mut(old_lists_index) };

		list.shuffle_song();
		if let Err((path, whys)) = list.load_song() {
			let (file_why, info_why) = (
				whys
					.0
					.map(move |why| format!("{why}"))
					.unwrap_or_default(),
				whys
					.1
					.map(move |why| format!("{why}"))
					.unwrap_or_default(),
			);
			log!(err[path]: "loading [{path}]" => file_why info_why; break)
		}

		let bundle = initialisable_bundle.get_or_init(|| Bundle::with(is_tty || flags.should_spawn_headless()));

		if list.is_empty() { list.repeat_or_increment(&mut lists_index) }

		if list.play(bundle, &mut lists_index, &mut volume) { break }
		clear()
	}

	if let Some(controls) = initialisable_bundle
		.into_inner()
		.map(Bundle::take_controls)
		.flatten()
	{
		controls.notify_exit();
		controls.clean_up();
	}
	if !flags.should_spawn_headless() {
		if let Err(why) = disable_raw_mode() { panic!("disable the raw mode of the current terminal  {why}") }
	}
	exit()
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
