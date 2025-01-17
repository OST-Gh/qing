///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use super::Error;
use crossterm::{
	execute,
	terminal::{Clear, ClearType},
};
use std::{
	env::var,
	io::stdout,
	path::{PathBuf, MAIN_SEPARATOR_STR},
};
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Format a text representation of a path into an absolute path.
///
/// This recursive function is used for unexpanded shell(zsh based) expressions, on a call site, and songs' file fields.
/// It can currently only expand environment variables, which might recurs.
pub fn fmt_path(path: impl AsRef<str>) -> Result<PathBuf, Error> {
	fn expand(name: &str) -> Result<String, Error> {
		let mut buffer = Vec::new();
		for part in var(if let Some(stripped) = name.strip_prefix('$') {
			expand(stripped)?
		} else {
			String::from(name)
		})?
		.split(MAIN_SEPARATOR_STR)
		.map(|part| {
			if let Some(stripped) = name.strip_prefix('$') {
				expand(stripped)
			} else {
				Ok(String::from(part))
			}
		}) {
			buffer.push(part?)
		}
		Ok(buffer.join(MAIN_SEPARATOR_STR))
	}

	let path = path.as_ref();
	Ok(PathBuf::from(
		path.split(MAIN_SEPARATOR_STR)
			.enumerate()
			.map(
				|(index, part)| match part {
					"~" if index == 0 => expand("HOME"),
					_ if part.starts_with('$') => expand(&part[1..]),
					_ => Ok(String::from(part)),
				}, //log!(part; "expanding [{}] to a path" why; None)
			)
			.collect::<Result<Vec<String>, Error>>()?
			.join(MAIN_SEPARATOR_STR),
	)
	.canonicalize()?)
}

/// Print the clear line sequence.
pub fn clear() -> Result<(), Error> {
	execute!(stdout(), Clear(ClearType::CurrentLine)).map_err(Error::Io)?;
	print!("\r");
	Ok(())
}
