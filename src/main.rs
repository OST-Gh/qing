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
	io::stdout,
	path::{ MAIN_SEPARATOR_STR, PathBuf },
	time::{ Duration, Instant },
	env::{ VarError, var },
};
use crossterm::{
	cursor::Hide,
	execute,
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
			print!(concat!("\rA non-fatal error occurred whilst attempting to ", $message, ';') $(, $($visible = $visible),+)?);
			$(print!(" '{}'", format!("{}", $why).replace('\n', "\r\n"));)+
			print!("\0\n");
			$($($retaliation)+)?
		}
	};
	(info$([$($visible: ident)+])?: $message: literal) => { print!(concat!('\r', $message, "\0\n") $(, $($visible = $visible),+)?) };
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
					Err(why) => log!(err[part]: "expand the shell expression [{part}] to a path" => why; None)
				}
			)
			.collect::<Vec<String>>()
			.join(MAIN_SEPARATOR_STR)
	)
		.canonicalize()
		.unwrap_or_else(|why| log!(err[path]: "canonicalise the path [{path}]" => why; PathBuf::new()))
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
				print!("\rAn error occurred whilst attempting to {message}; '{reason}'\0\n");
				exit();
			}
		)
	);

	let (flags, files) = Flags::new();
	if !flags.should_spawn_headless() {
		if let Err(why) = enable_raw_mode() { panic!("enable the raw mode of the current terminal  {why}") }
		if let Err(why) = execute!(stdout(),
			Hide,
			SetForegroundColor(Color::Yellow),
		) { log!(err: "set the terminal style" => why) }
	}

	if flags.should_print_version() { println!(concat!(env!("CARGO_PKG_NAME"), " on version ", env!("CARGO_PKG_VERSION"), " by ", env!("CARGO_PKG_AUTHORS"), '.')) }

	let mut lists = files
		.filter_map(|path|
			{
				log!(info[path]: "Loading and parsing data from [{path}].");
				Playlist::try_from_path(path)
			}
		)
		.collect();
	print!("\r\n\n\0");

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

		let name = list.get_name();

		log!(info[name]: "Shuffling all of the songs in [{name}].");
		list.shuffle_song();

		log!(info[name]: "Loading all of the audio contents of the songs in [{name}].");
		list.load_song();
		print!("\r\n\0");

		let bundle = initialisable_bundle.get_or_init(|| if flags.should_spawn_headless() { Bundle::headless() } else { Bundle::new() });

		if list.play(bundle, &mut lists_index, &mut volume) { break };
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
