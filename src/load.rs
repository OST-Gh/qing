///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	io::BufReader,
	fs::{ File, read_to_string },
};
use super::{
	Duration,
	Track,
	Playlist,
	log,
	fmt_path,
};
use lofty::{ read_from_path, AudioFile };
use toml::from_str;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Global audio stream data.
pub(crate) static mut FILES: Vec<BufReader<File>> = Vec::new();
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Apply a function to the files.
pub(crate) fn map_files<O>(function: impl FnOnce(&mut Vec<BufReader<File>>) -> O) -> O {
	unsafe { function(&mut FILES) }
}

/// Get the file at the given index.
///
/// Panics:
///
/// The function does not panic, but it does not guarrante that the index is inside the bounds of the static.
pub(crate) fn get_file(index: usize) -> &'static mut BufReader<File> {
	unsafe { FILES.get_unchecked_mut(index) }
}

/// Load songs from song metadata and playlist name.
pub(crate) fn tracks(name: &str, play_list: &[Track]) -> Vec<(Box<str>, Duration, isize)> {
	log!(info[name]: "Loading all of the audio contents of the songs in [{name}].");
	let result = play_list
		.iter()
		.filter_map(|Track { name, file, time }|
			{
				let name = name
					.clone()
					.unwrap_or_default();
				let formatted = fmt_path(file);
				match (File::open(&formatted), read_from_path(formatted)) {
					(Ok(contents), Ok(info)) => {
						unsafe { FILES.push(BufReader::new(contents)) }
						return Some(info.properties()).map(|info| (name, info.duration(), time.unwrap_or(0)))
					},
					(Err(why), Ok(_)) => log!(err[name]: "load the audio contents of [{name}]" => why),
					(Ok(_), Err(why)) => log!(err[name]: "load the audio properties of [{name}]" => why),
					(Err(file_why), Err(info_why)) => log!(err[name]: "load the audio contents and properties of [{name}]" => file_why info_why),
				}
				None
			}
		)
		.collect();
	print!("\r\n\0");
	result
}

/// Load playlists from paths.
pub(crate) fn playlists(list_list: impl Iterator<Item = String>) -> Vec<(Box<str>, Vec<Track>, isize)>  {
	let result = list_list
		.filter_map(|path|
			{
				log!(info[path]: "Loading and parsing data from [{path}].");
				let Playlist { song, name, time } = match read_to_string(fmt_path(&path)).map(|contents| from_str(&contents)) {
					Ok(Ok(playlist)) => playlist,
					Ok(Err(why)) => log!(err[path]: "parse the contents of [{path}]" => why; None?),
					Err(why) => log!(err[path]: "load the contents of [{path}]" => why; None?),
				};
				Some((name.unwrap_or_default(), song, time.unwrap_or_default()))
			}
		)
		.collect();
	print!("\r\n\n\0");
	result
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
